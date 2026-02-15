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

## Troubleshooting

### Database Error: `Peer authentication failed`
If you see this error, it means you can't log in as `postgres` without sudo. To fix:
1.  **Set a password** for the `postgres` user:
    ```bash
    sudo -u postgres psql -c "ALTER USER postgres PASSWORD 'yourpassword';"
    ```
2.  Update `.env` with:
    ```
    DATABASE_URL=postgres://postgres:yourpassword@localhost:5432/tts_stt_bot
    ```

### Build Error: `linking with cc failed`
Ensure you have `libclang-dev` (Ubuntu) or `clang-devel` (Fedora) and `ffmpeg` installed.

**Fedora:**
```bash
sudo dnf install cmake pkg-config gcc-c++ clang-devel llvm-devel openssl-devel ffmpeg-free-devel
```

**Ubuntu/Debian:**
```bash
sudo apt install libclang-dev libssl-dev pkg-config build-essential ffmpeg
```
