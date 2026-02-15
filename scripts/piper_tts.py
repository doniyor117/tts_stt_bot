#!/usr/bin/env python3
"""Piper TTS wrapper. Reads text from stdin, outputs raw PCM s16le 22050Hz mono to stdout."""
import sys

def main():
    model_path = sys.argv[1] if len(sys.argv) > 1 else None
    if not model_path:
        print("Usage: piper_tts.py <model_path>", file=sys.stderr)
        sys.exit(1)

    text = sys.stdin.read().strip()
    if not text:
        sys.exit(0)

    from piper import PiperVoice

    voice = PiperVoice.load(model_path)

    # synthesize_stream_raw yields chunks of raw PCM s16le audio
    for audio_bytes in voice.synthesize_stream_raw(text):
        sys.stdout.buffer.write(audio_bytes)

if __name__ == "__main__":
    main()
