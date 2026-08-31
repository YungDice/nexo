//! Link previews (§4.5 and §6.4), fetched by this machine and nobody else.
//!
//! # Why this is off by default, and why it is in Rust
//!
//! A preview means someone's computer fetches a URL that arrived in a message.
//! Doing that **on the server** would be the worst of both worlds: it would
//! tell the server which links its users are reading — precisely the metadata
//! the rest of the app refuses to hold — and it would turn the server into a
//! request forwarder for anyone who can send a message, which is server-side
//! request forgery by design. So the fetch happens here.
//!
//! Doing it here has its own cost, and the setting is off by default because
//! of it: fetching a link reveals this machine's IP address and rough activity
//! to whoever controls the URL. A sender can learn "you opened the
//! conversation" by planting a link. That is a real trade, so it is a choice
//! rather than a default, and the Settings copy says what it costs.
//!
//! # What this module refuses to do
//!
//! The URL comes from a message, which means it comes from someone else. Every
//! restriction below exists because of that:
//!
//! - **`https` only.** A plaintext fetch would leak the URL to the network as
//!   well as to its owner.
//! - **No private, loopback or link-local addresses**, checked after DNS
//!   resolution rather than by pattern-matching the hostname — the string
//!   `notprivate.example.com` can resolve to `127.0.0.1`, and only resolving
//!   catches it. This is what stops a message from making the app probe the
//!   machine's own network.
//! - **No redirects.** A permitted URL that redirects to a forbidden one would
//!   walk straight past the check above.
//! - **A byte ceiling and a timeout**, so a hostile endpoint cannot hand back
//!   an endless stream or hold the connection open.
//! - **Text only.** Anything that is not HTML is not parsed.

use std::net::{IpAddr, ToSocketAddrs};
use std::time::Duration;

/// What a preview says. Text only: no image is fetched.
///
/// Fetching the OpenGraph image would be a second request to the same
/// operator, for a thumbnail the bubble draws as a generated field anyway.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PreviewView {
    /// The URL that was fetched, as given.
    pub url: String,
    /// The page title, trimmed and length-capped.
    pub title: String,
    /// The meta description, or empty.
    pub description: String,
    /// The host, for the small line above the title.
    pub source: String,
}

/// How long the whole fetch may take.
const TIMEOUT: Duration = Duration::from_secs(6);

/// How much of the response is read before giving up on finding a title.
///
/// `<head>` is at the top of any document that has one, so a page whose title
/// is past this is not a page with a usable preview.
const MAX_BYTES: usize = 256 * 1024;

/// Longest strings kept, in characters.
const MAX_TITLE: usize = 140;
const MAX_DESCRIPTION: usize = 300;

/// Why a preview could not be made.
#[derive(Debug, thiserror::Error)]
pub enum PreviewError {
    #[error("only https links are previewed")]
    NotHttps,
    #[error("that link has no host")]
    NoHost,
    #[error("that link points at a private address")]
    PrivateAddress,
    #[error("the link could not be fetched")]
    Unreachable,
    #[error("that link is not a web page")]
    NotHtml,
    #[error("that page has no title")]
    NoTitle,
}

/// Splits a URL into scheme, host, and port, without a URL crate.
///
/// Deliberately strict: anything this cannot parse confidently is refused
/// rather than guessed at, because the result decides what gets fetched.
fn parse(url: &str) -> Result<(String, u16), PreviewError> {
    let rest = url.strip_prefix("https://").ok_or(PreviewError::NotHttps)?;
    // Authority ends at the first '/', '?' or '#'.
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .ok_or(PreviewError::NoHost)?;
    // Userinfo in a link from a stranger is a phishing shape (`https://
    // trusted.example@attacker.example`), and nothing legitimate needs it.
    if authority.contains('@') || authority.is_empty() {
        return Err(PreviewError::NoHost);
    }
    // No IPv6 literals: they are always either loopback, link-local, or an
    // address someone typed to bypass a hostname check.
    if authority.contains('[') {
        return Err(PreviewError::PrivateAddress);
    }
    match authority.split_once(':') {
        Some((host, port)) => {
            let port = port.parse().map_err(|_| PreviewError::NoHost)?;
            if host.is_empty() {
                return Err(PreviewError::NoHost);
            }
            Ok((host.to_string(), port))
        }
        None => Ok((authority.to_string(), 443)),
    }
}

/// True for an address the app must not reach on someone else's instruction.
///
/// The unspecified address is included because `0.0.0.0` routes to localhost
/// on Windows, which is exactly the bypass this function exists to close.
fn is_forbidden(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                // 100.64.0.0/10, carrier-grade NAT: not routable on the
                // public internet and often a private network in practice.
                || (v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]))
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // fc00::/7 unique-local and fe80::/10 link-local.
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Resolves the host and refuses if any address it answers with is private.
///
/// *Any*, not *the first*: a name that resolves to both a public and a private
/// address would otherwise be a coin flip, and a hostile one would simply
/// order the records to win it.
fn resolve_is_public(host: &str, port: u16) -> Result<(), PreviewError> {
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|_| PreviewError::Unreachable)?;
    let mut any = false;
    for address in addresses {
        any = true;
        if is_forbidden(address.ip()) {
            return Err(PreviewError::PrivateAddress);
        }
    }
    if any {
        Ok(())
    } else {
        Err(PreviewError::Unreachable)
    }
}

/// Pulls the title, description and host out of an HTML document.
///
/// A deliberately small parser rather than a scraping crate: the whole job is
/// three fields out of `<head>`, and every dependency in this process is one
/// more thing that parses hostile input.
pub fn extract(html: &str, url: &str, host: &str) -> Result<PreviewView, PreviewError> {
    let title = meta_content(html, "og:title")
        .or_else(|| tag_text(html, "title"))
        .map(|t| clamp(&decode_entities(&t), MAX_TITLE))
        .filter(|t| !t.is_empty())
        .ok_or(PreviewError::NoTitle)?;

    let description = meta_content(html, "og:description")
        .or_else(|| meta_named(html, "description"))
        .map(|d| clamp(&decode_entities(&d), MAX_DESCRIPTION))
        .unwrap_or_default();

    Ok(PreviewView {
        url: url.to_string(),
        title,
        description,
        source: host.to_string(),
    })
}

/// The text of the first `<tag>…</tag>`, with tags inside it stripped.
fn tag_text(html: &str, tag: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open = lower.find(&format!("<{tag}"))?;
    let after_open = lower[open..].find('>')? + open + 1;
    let close = lower[after_open..].find(&format!("</{tag}"))? + after_open;
    let raw = &html[after_open..close];
    // A title containing markup is malformed, but it happens; take the text.
    let text: String = strip_tags(raw);
    Some(text.trim().to_string())
}

fn strip_tags(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut inside = false;
    for c in raw.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    out
}

/// `content` of the `<meta property="og:…">` with the given property.
fn meta_content(html: &str, property: &str) -> Option<String> {
    meta_with(html, "property", property).or_else(|| meta_with(html, "name", property))
}

/// `content` of `<meta name="description">`.
fn meta_named(html: &str, name: &str) -> Option<String> {
    meta_with(html, "name", name)
}

/// Finds a `<meta>` whose `key` attribute equals `value` and returns `content`.
///
/// Attribute order is not fixed in real documents, so this looks at the whole
/// tag rather than assuming `key` comes first.
fn meta_with(html: &str, key: &str, value: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut cursor = 0;
    while let Some(start) = lower[cursor..].find("<meta") {
        let start = cursor + start;
        let end = lower[start..].find('>').map(|e| start + e)?;
        let tag = &html[start..end];
        let tag_lower = &lower[start..end];

        let matches_key = attribute(tag_lower, key)
            .map(|v| v.eq_ignore_ascii_case(value))
            .unwrap_or(false);
        if matches_key
            && let Some(content) = attribute(tag, "content")
            && !content.trim().is_empty()
        {
            return Some(content.trim().to_string());
        }
        cursor = end + 1;
    }
    None
}

/// The quoted value of one attribute within a single tag.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    loop {
        let at = lower[from..].find(name)? + from;
        // Must be preceded by whitespace, or it is the tail of another
        // attribute name (`data-name` would otherwise match `name`).
        let preceded_ok = at == 0
            || lower[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        let after = &lower[at + name.len()..];
        let trimmed = after.trim_start();
        if preceded_ok && trimmed.starts_with('=') {
            let value_part = &tag[at + name.len()..];
            let value_part = value_part.trim_start().strip_prefix('=')?.trim_start();
            let quote = value_part.chars().next()?;
            if quote == '"' || quote == '\'' {
                let rest = &value_part[1..];
                let end = rest.find(quote)?;
                return Some(rest[..end].to_string());
            }
            // Unquoted: runs to whitespace.
            let end = value_part
                .find(char::is_whitespace)
                .unwrap_or(value_part.len());
            return Some(value_part[..end].to_string());
        }
        from = at + name.len();
    }
}

/// The handful of entities that actually appear in titles.
fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

/// Trims, collapses whitespace, and caps the length.
///
/// Collapsing matters: a `<title>` split across lines in the source would
/// otherwise arrive in a chat bubble with its newlines intact.
fn clamp(text: &str, max: usize) -> String {
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        return collapsed;
    }
    let mut out: String = collapsed.chars().take(max).collect();
    out.push('…');
    out
}

/// Fetches a link and builds its preview.
///
/// Every refusal above happens before a byte is sent.
pub fn fetch(url: &str) -> Result<PreviewView, PreviewError> {
    let (host, port) = parse(url)?;
    resolve_is_public(&host, port)?;

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        // No redirects: a permitted URL that redirects to a private address
        // would walk straight past the resolution check above.
        .max_redirects(0)
        .build()
        .into();

    let mut response = agent
        .get(url)
        .header("accept", "text/html")
        // Identifying honestly. A spoofed browser agent would be a small lie
        // told to every site someone links, for no benefit.
        .header("user-agent", "Nexo/0.1 (+link preview; desktop client)")
        .call()
        .map_err(|_| PreviewError::Unreachable)?;

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !content_type.contains("text/html") && !content_type.contains("application/xhtml") {
        return Err(PreviewError::NotHtml);
    }

    let html = response
        .body_mut()
        .with_config()
        .limit(MAX_BYTES as u64)
        .read_to_string()
        .map_err(|_| PreviewError::Unreachable)?;

    extract(&html, url, &host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_is_previewed() {
        // http would leak the URL to the network as well as to its owner.
        assert!(matches!(
            parse("http://example.com/x"),
            Err(PreviewError::NotHttps)
        ));
        assert!(matches!(
            parse("ftp://example.com"),
            Err(PreviewError::NotHttps)
        ));
        assert!(matches!(
            parse("file:///c:/windows"),
            Err(PreviewError::NotHttps)
        ));
    }

    #[test]
    fn the_host_is_read_without_the_path() {
        assert_eq!(
            parse("https://example.com/a/b?c#d").unwrap(),
            ("example.com".into(), 443)
        );
        assert_eq!(
            parse("https://example.com:8443/x").unwrap(),
            ("example.com".into(), 8443)
        );
    }

    #[test]
    fn userinfo_is_refused_because_it_is_a_phishing_shape() {
        // https://trusted.example@attacker.example fetches the attacker.
        assert!(matches!(
            parse("https://trusted.example@attacker.example/"),
            Err(PreviewError::NoHost)
        ));
    }

    #[test]
    fn ipv6_literals_are_refused() {
        assert!(matches!(
            parse("https://[::1]/"),
            Err(PreviewError::PrivateAddress)
        ));
    }

    #[test]
    fn private_and_loopback_addresses_are_forbidden() {
        // The whole point: a message must not be able to make this machine
        // probe its own network.
        for address in [
            "127.0.0.1",
            "10.0.0.5",
            "192.168.1.1",
            "172.16.0.1",
            "169.254.1.1",
            "0.0.0.0",
            "100.64.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
        ] {
            assert!(
                is_forbidden(address.parse().unwrap()),
                "{address} should be forbidden"
            );
        }
    }

    #[test]
    fn public_addresses_are_allowed() {
        for address in ["1.1.1.1", "93.184.216.34", "2606:4700:4700::1111"] {
            assert!(
                !is_forbidden(address.parse().unwrap()),
                "{address} should be allowed"
            );
        }
    }

    #[test]
    fn a_resolving_localhost_name_is_still_refused() {
        // The reason the check is on resolved addresses rather than on the
        // hostname string: this name is not "localhost", and it points there.
        assert!(matches!(
            resolve_is_public("localhost", 443),
            Err(PreviewError::PrivateAddress)
        ));
    }

    #[test]
    fn a_title_tag_is_enough() {
        let html = "<html><head><title>Hello there</title></head><body>x</body></html>";
        let preview = extract(html, "https://example.com/", "example.com").unwrap();
        assert_eq!(preview.title, "Hello there");
        assert_eq!(preview.description, "");
        assert_eq!(preview.source, "example.com");
    }

    #[test]
    fn opengraph_wins_over_the_title_tag() {
        // og:title is what the author chose for sharing; <title> often carries
        // a site-name suffix nobody wants in a chat bubble.
        let html = r#"<head><title>Page — Site</title>
            <meta property="og:title" content="Page">
            <meta property="og:description" content="What it is."></head>"#;
        let preview = extract(html, "https://example.com/", "example.com").unwrap();
        assert_eq!(preview.title, "Page");
        assert_eq!(preview.description, "What it is.");
    }

    #[test]
    fn a_meta_description_is_found_whatever_the_attribute_order() {
        let html = r#"<head><title>T</title>
            <meta content="Ordered the other way" name="description"></head>"#;
        let preview = extract(html, "https://example.com/", "example.com").unwrap();
        assert_eq!(preview.description, "Ordered the other way");
    }

    #[test]
    fn a_page_with_no_title_is_not_a_preview() {
        assert!(matches!(
            extract(
                "<html><body>nothing</body></html>",
                "https://e.com/",
                "e.com"
            ),
            Err(PreviewError::NoTitle)
        ));
        // And an empty one is the same as none.
        assert!(matches!(
            extract("<head><title>   </title></head>", "https://e.com/", "e.com"),
            Err(PreviewError::NoTitle)
        ));
    }

    #[test]
    fn markup_inside_a_title_does_not_reach_the_bubble() {
        // The title is rendered as text, but stripping here keeps the model
        // clean rather than relying on every renderer to be careful.
        let html = "<head><title>Hi <b>there</b> <script>x</script></title></head>";
        let preview = extract(html, "https://e.com/", "e.com").unwrap();
        assert_eq!(preview.title, "Hi there x");
        assert!(!preview.title.contains('<'));
    }

    #[test]
    fn entities_are_decoded_and_whitespace_collapsed() {
        let html = "<head><title>Tom\n  &amp;\n  Jerry</title></head>";
        let preview = extract(html, "https://e.com/", "e.com").unwrap();
        assert_eq!(preview.title, "Tom & Jerry");
    }

    #[test]
    fn long_text_is_capped_rather_than_filling_the_conversation() {
        let long = "a".repeat(500);
        let html = format!("<head><title>{long}</title></head>");
        let preview = extract(&html, "https://e.com/", "e.com").unwrap();
        assert_eq!(preview.title.chars().count(), MAX_TITLE + 1); // plus the ellipsis
        assert!(preview.title.ends_with('…'));
    }

    #[test]
    fn a_data_name_attribute_is_not_mistaken_for_name() {
        let html = r#"<head><title>T</title>
            <meta data-name="description" content="not this">
            <meta name="description" content="this one"></head>"#;
        let preview = extract(html, "https://e.com/", "e.com").unwrap();
        assert_eq!(preview.description, "this one");
    }
}
