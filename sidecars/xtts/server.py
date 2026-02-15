"""
XTTS-v2 Sidecar Server
A minimal FastAPI server that wraps XTTS-v2 for use by the Rust bot.
Run: python server.py
"""
import io
import os

# PyTorch 2.6+ defaults weights_only=True which breaks XTTS checkpoint loading.
# Patch torch.load before importing TTS to allow loading the trusted Coqui model.
import torch
_original_torch_load = torch.load
def _patched_torch_load(*args, **kwargs):
    kwargs.setdefault("weights_only", False)
    return _original_torch_load(*args, **kwargs)
torch.load = _patched_torch_load

from fastapi import FastAPI
from fastapi.responses import StreamingResponse
from pydantic import BaseModel
from TTS.api import TTS

app = FastAPI(title="XTTS-v2 Sidecar")

# Load model on startup
tts = None

# Default speaker for XTTS-v2 multi-speaker model
DEFAULT_SPEAKER = os.environ.get("XTTS_SPEAKER", "Claribel Dervla")

@app.on_event("startup")
async def load_model():
    global tts
    print("Loading XTTS-v2 model...")
    tts = TTS("tts_models/multilingual/multi-dataset/xtts_v2")
    if os.environ.get("USE_GPU", "0") == "1":
        tts.to("cuda")
    print(f"XTTS-v2 model loaded! Default speaker: {DEFAULT_SPEAKER}")
    # Print available speakers for reference
    if hasattr(tts, "speakers") and tts.speakers:
        print(f"Available speakers: {len(tts.speakers)}")

class TTSRequest(BaseModel):
    text: str
    language: str = "en"
    speaker: str | None = None
    speaker_wav: str | None = None

@app.post("/tts")
async def generate_speech(req: TTSRequest):
    """Generate speech from text, return WAV audio."""
    wav_buffer = io.BytesIO()

    speaker = req.speaker or DEFAULT_SPEAKER

    try:
        if req.speaker_wav and os.path.exists(req.speaker_wav):
            # Voice cloning mode
            tts.tts_to_file(
                text=req.text,
                language=req.language,
                speaker_wav=req.speaker_wav,
                file_path=wav_buffer,
            )
        else:
            # Use named speaker
            tts.tts_to_file(
                text=req.text,
                language=req.language,
                speaker=speaker,
                file_path=wav_buffer,
            )
    except Exception as e:
        from fastapi.responses import JSONResponse
        return JSONResponse(
            status_code=500,
            content={"error": str(e)},
        )

    wav_buffer.seek(0)
    return StreamingResponse(wav_buffer, media_type="audio/wav")

@app.get("/health")
async def health():
    return {"status": "ok", "model": "xtts-v2", "speaker": DEFAULT_SPEAKER}

@app.get("/speakers")
async def list_speakers():
    """List available speakers."""
    if hasattr(tts, "speakers") and tts.speakers:
        return {"speakers": tts.speakers}
    return {"speakers": []}

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8020)
