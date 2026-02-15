# TTS/STT Agentic Bot 🤖🎙️

A high-performance voice and text chatbot built in **Rust**, featuring:
- **Speech-to-Text** via `whisper-rs` (whisper.cpp bindings)
- **Text-to-Speech** via **Piper** (fast, CPU) and **XTTS-v2** (quality, GPU)
- **LLM** via **Groq API** (Llama 3 / Mixtral)
- **Telegram** interface via `teloxide`
- **PostgreSQL** for conversation history and user profiles
- **Agentic** tool use with admin approval for risky commands

## Quick Start

### Prerequisites
- Rust toolchain (`rustup`)
- PostgreSQL
- `ffmpeg` (for audio conversion)
- `piper` CLI (for Piper TTS)
- `libclang-dev`, `cmake` (for building whisper.cpp)

### Setup

1. **Clone and configure:**
   ```bash
   cp .env.example .env
   # Edit .env with your keys
   ```

2. **Download models:**
   ```bash
   # Whisper model
   mkdir -p data/models/whisper
   wget -O data/models/whisper/ggml-base.en.bin \
     https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin

   # Piper model
   mkdir -p data/models/piper
   # Download from https://github.com/rhasspy/piper/blob/master/VOICES.md
   ```

3. **Build and run:**
   ```bash
   cargo build --release
   cargo run --release
   ```

4. **(Optional) XTTS Sidecar:**
   ```bash
   cd sidecars/xtts
   pip install -r requirements.txt
   python server.py
   ```

## Commands

| Command | Description |
|---------|-------------|
| `/start` | Initialize the bot |
| `/new` | Start a new conversation |
| `/history` | Browse past conversations |
| `/settings` | Configure TTS engine |
| `/help` | Show available commands |

## Architecture

```
User → Telegram → Teloxide Handler
                      ↓
              Voice? → ffmpeg → whisper-rs → text
                      ↓
              Context Manager (auto-prune/summarize)
                      ↓
              System Prompt (SOUL + IDENTITY + SECURITY + User Profile)
                      ↓
              Groq API → Response
                      ↓
              Tool Call? → Executor → (Admin Approval if risky)
                      ↓
              TTS (Piper/XTTS) → Voice Reply
```
