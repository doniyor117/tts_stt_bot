"""
XTTS-v2 Sidecar Server
A minimal FastAPI server that wraps XTTS-v2 for use by the Rust bot.
Run: python server.py
"""
import io
import os
from fastapi import FastAPI
from fastapi.responses import StreamingResponse
from pydantic import BaseModel
from TTS.api import TTS

app = FastAPI(title="XTTS-v2 Sidecar")

# Load model on startup
tts = None

@app.on_event("startup")
async def load_model():
    global tts
    print("Loading XTTS-v2 model...")
    tts = TTS("tts_models/multilingual/multi-dataset/xtts_v2")
    if os.environ.get("USE_GPU", "0") == "1":
        tts.to("cuda")
    print("XTTS-v2 model loaded!")

class TTSRequest(BaseModel):
    text: str
    language: str = "en"
    speaker_wav: str | None = None

@app.post("/tts")
async def generate_speech(req: TTSRequest):
    """Generate speech from text, return WAV audio."""
    wav_buffer = io.BytesIO()

    if req.speaker_wav and os.path.exists(req.speaker_wav):
        tts.tts_to_file(
            text=req.text,
            language=req.language,
            speaker_wav=req.speaker_wav,
            file_path=wav_buffer,
        )
    else:
        tts.tts_to_file(
            text=req.text,
            language=req.language,
            file_path=wav_buffer,
        )

    wav_buffer.seek(0)
    return StreamingResponse(wav_buffer, media_type="audio/wav")

@app.get("/health")
async def health():
    return {"status": "ok", "model": "xtts-v2"}

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8020)
