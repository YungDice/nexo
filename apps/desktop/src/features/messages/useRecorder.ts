import { useCallback, useEffect, useRef, useState } from "react";

/**
 * Recording a voice message, and the waveform that describes it.
 *
 * `MediaRecorder` is the only capture this app has without taking a native
 * audio dependency, which would have to be written twice — once for Windows and
 * again for the Android port — for a feature that is the same on both. So the
 * bytes are made here and handed straight to Rust, which encrypts and uploads
 * them; see `sendVoiceMessage`.
 *
 * The peaks are computed **while recording**, from the analyser, rather than by
 * decoding the finished file. Decoding a WebM/Opus blob to draw sixty-four bars
 * would mean holding the decoded PCM — several megabytes for a minute of speech
 * — for a picture. Sampling as it goes costs one float array that never grows.
 */

/** How many bars a full-length recording is drawn with. */
const BARS = 64;

/** How often the analyser is read, in milliseconds. */
const SAMPLE_MS = 50;

export type RecorderState = "idle" | "asking" | "recording" | "denied";

export interface Recording {
  blob: Blob;
  durationMs: number;
  /** `0`-`255` per bar, at most `BARS` of them. */
  peaks: number[];
}

export interface Recorder {
  state: RecorderState;
  /** Milliseconds so far, for the running timer. */
  elapsedMs: number;
  /** What has been sampled so far, so the bars grow as you speak. */
  peaks: number[];
  start: () => Promise<void>;
  /** Stops and resolves with the recording, or `null` if nothing was captured. */
  stop: () => Promise<Recording | null>;
  /** Stops and throws the recording away. */
  cancel: () => void;
}

/**
 * Reduces however many samples were taken to at most [`BARS`] bars.
 *
 * A ten-second note is two hundred samples and a two-second one is forty, and
 * both should fill the same width — so this averages into buckets rather than
 * truncating. Fewer samples than bars are left alone; stretching four samples
 * across sixty-four bars would draw a detail that was never measured.
 */
function toBars(samples: number[]): number[] {
  if (samples.length <= BARS) return samples.map((s) => Math.round(s));
  const bars: number[] = [];
  const per = samples.length / BARS;
  for (let i = 0; i < BARS; i += 1) {
    const from = Math.floor(i * per);
    const to = Math.max(from + 1, Math.floor((i + 1) * per));
    let sum = 0;
    // `noUncheckedIndexedAccess` is on, and the bounds above already keep this
    // inside the array -- the fallback is the type system's price, not a case.
    for (let j = from; j < to; j += 1) sum += samples[j] ?? 0;
    bars.push(Math.round(sum / (to - from)));
  }
  return bars;
}

export function useRecorder(): Recorder {
  const [state, setState] = useState<RecorderState>("idle");
  const [elapsedMs, setElapsedMs] = useState(0);
  const [peaks, setPeaks] = useState<number[]>([]);

  const recorder = useRef<MediaRecorder | null>(null);
  const stream = useRef<MediaStream | null>(null);
  const audioContext = useRef<AudioContext | null>(null);
  const chunks = useRef<Blob[]>([]);
  const samples = useRef<number[]>([]);
  const startedAt = useRef(0);
  const timer = useRef<number | null>(null);
  const sampler = useRef<number | null>(null);

  /**
   * Everything this hook holds open, released together.
   *
   * The microphone track especially: leaving it live keeps the recording
   * indicator lit in Windows after the app thinks it has stopped, which reads
   * as the app listening when it is not.
   */
  const teardown = useCallback(() => {
    if (timer.current !== null) window.clearInterval(timer.current);
    if (sampler.current !== null) window.clearInterval(sampler.current);
    timer.current = null;
    sampler.current = null;
    stream.current?.getTracks().forEach((track) => track.stop());
    stream.current = null;
    void audioContext.current?.close();
    audioContext.current = null;
    recorder.current = null;
  }, []);

  // A window closed mid-recording must not leave the microphone open.
  useEffect(() => teardown, [teardown]);

  const start = useCallback(async () => {
    setState("asking");
    let media: MediaStream;
    try {
      media = await navigator.mediaDevices.getUserMedia({ audio: true });
    } catch {
      // Denied, or there is no microphone. Both end the same way: the control
      // says why rather than failing silently when it is pressed again.
      setState("denied");
      return;
    }

    stream.current = media;
    chunks.current = [];
    samples.current = [];
    setPeaks([]);
    setElapsedMs(0);

    const context = new AudioContext();
    audioContext.current = context;
    const analyser = context.createAnalyser();
    analyser.fftSize = 512;
    context.createMediaStreamSource(media).connect(analyser);
    const buffer = new Uint8Array(analyser.frequencyBinCount);

    const media_recorder = new MediaRecorder(media);
    recorder.current = media_recorder;
    media_recorder.ondataavailable = (event) => {
      if (event.data.size > 0) chunks.current.push(event.data);
    };
    media_recorder.start();
    startedAt.current = Date.now();

    timer.current = window.setInterval(() => {
      setElapsedMs(Date.now() - startedAt.current);
    }, 100);

    sampler.current = window.setInterval(() => {
      analyser.getByteTimeDomainData(buffer);
      // The analyser gives 0-255 centred on 128. Loudness is the distance from
      // that centre, doubled to use the whole range a byte can hold.
      let loudest = 0;
      for (const value of buffer) {
        const distance = Math.abs(value - 128);
        if (distance > loudest) loudest = distance;
      }
      samples.current.push(Math.min(255, loudest * 2));
      setPeaks(toBars(samples.current));
    }, SAMPLE_MS);

    setState("recording");
  }, []);

  const stop = useCallback(async (): Promise<Recording | null> => {
    const media_recorder = recorder.current;
    if (!media_recorder || state !== "recording") {
      teardown();
      setState("idle");
      return null;
    }

    const durationMs = Date.now() - startedAt.current;
    const bars = toBars(samples.current);

    const blob = await new Promise<Blob>((resolve) => {
      media_recorder.onstop = () => {
        resolve(new Blob(chunks.current, { type: media_recorder.mimeType }));
      };
      media_recorder.stop();
    });

    teardown();
    setState("idle");
    setElapsedMs(0);
    setPeaks([]);

    // A tap rather than a hold. Nothing was said, so nothing is sent — better
    // than posting a quarter-second of room tone somebody has to delete.
    if (blob.size === 0 || durationMs < 400) return null;
    return { blob, durationMs, peaks: bars };
  }, [state, teardown]);

  const cancel = useCallback(() => {
    // `stop()` on the recorder still fires `ondataavailable`, but nothing is
    // listening for it once this has run and the chunks go with the teardown.
    try {
      recorder.current?.stop();
    } catch {
      // Already stopped. There is nothing to recover from here.
    }
    chunks.current = [];
    samples.current = [];
    teardown();
    setState("idle");
    setElapsedMs(0);
    setPeaks([]);
  }, [teardown]);

  return { state, elapsedMs, peaks, start, stop, cancel };
}

/** `m:ss`, for the running timer and the finished bubble. */
export function formatDuration(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000));
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}
