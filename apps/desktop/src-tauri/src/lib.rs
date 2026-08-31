//! The Nexo desktop core.
//!
//! Everything security-relevant lives on this side of the IPC boundary: MLS
//! state, the identity keypair, the SQLCipher key, and every message plaintext.
//! The WebView receives already-decrypted strings and nothing else (rule 2).

#![forbid(unsafe_code)]

mod auth;
mod client;
mod commands;
mod conversations;
mod feed;
mod preview;
mod windows;

/// Start the app.
pub fn run() {
    init_tracing();

    tauri::Builder::default()
        // First, before anything else can take the port or the lock: a second
        // launch hands its arguments to the running instance and exits (§8).
        // Two instances would mean two SQLCipher connections to one file and
        // two MLS providers ratcheting the same group forward independently --
        // the second is the one that corrupts state.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows::show_main_window(app);
        }))
        // The session lives here, in the Rust process. Tokens never cross the
        // IPC boundary (rule 2).
        .manage(auth::SessionState::default())
        .manage(windows::WindowPrefs::default())
        .manage(client::ClientState::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        // HKCU, never HKLM (§8). A per-machine Run key needs admin, affects
        // every account on the computer, and is not this app's to write.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // §8: manifests signed with the minisign key whose public half is
        // pinned in tauri.conf.json. The plugin refuses anything the key did
        // not sign, so the update server is not trusted, only the key.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            windows::install_tray(app.handle())?;
            // Close-to-tray defaults off (see `WindowPrefs`). Someone who
            // closes a window and finds the app still running has been
            // surprised by their own computer; they can turn it on in
            // Settings once they know it exists, and the WebView pushes the
            // stored preference across at startup.
            windows::install_close_to_tray(app.handle());
            Ok(())
        })
        // Commands are added one at a time, and each one needs a matching
        // entry in capabilities/default.json. The capability set starts empty
        // and only ever grows deliberately (§4.5).
        .invoke_handler(tauri::generate_handler![
            commands::app_version,
            commands::notify_message,
            commands::set_unread,
            commands::lock,
            commands::is_unlocked,
            commands::focus_window,
            commands::set_close_to_tray,
            commands::set_window_backdrop,
            commands::storage_info,
            commands::clear_media_cache,
            commands::preview_link,
            commands::get_autostart,
            commands::set_autostart,
            commands::check_update,
            commands::install_update,
            auth::register,
            auth::login,
            auth::restore_session,
            auth::change_password,
            auth::device_fingerprint,
            auth::pin_status,
            auth::set_pin,
            auth::clear_pin,
            auth::unlock_with_pin,
            auth::logout,
            conversations::list_conversations,
            conversations::delete_conversation,
            conversations::start_conversation,
            conversations::start_group,
            conversations::add_to_conversation,
            conversations::rename_conversation,
            conversations::mark_verified,
            conversations::acknowledge_key_change,
            conversations::set_conversation_avatar,
            conversations::conversation_avatar,
            conversations::attachment_data_url,
            conversations::conversation_attachments,
            conversations::send_message,
            conversations::sync_conversation,
            conversations::sync_all,
            conversations::conversation_messages,
            conversations::send_attachment,
            conversations::save_attachment,
            conversations::flush_outbox,
            conversations::outbox_count,
            conversations::safety_number,
            feed::feed,
            feed::posts_by,
            feed::create_post,
            feed::delete_post,
            feed::react,
            feed::pin_post,
            feed::unpin_post,
            feed::blocks,
            feed::block,
            feed::unblock,
            feed::profile,
            feed::my_profile,
            feed::update_profile,
            feed::update_visibility,
            feed::upload_image,
            feed::vote,
            feed::comments,
            feed::add_comment,
            feed::delete_comment,
            feed::image_url,
            feed::image_data_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running nexo");
}

fn init_tracing() {
    // §4.5: nothing above `debug` may contain user content, and `debug` is
    // compiled out of release builds. `debug_assertions` is the switch, and
    // the release profile in the workspace manifest turns it off.
    let default = if cfg!(debug_assertions) {
        "nexo_desktop_lib=debug"
    } else {
        "nexo_desktop_lib=info"
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| default.into()),
        )
        .init();
}
