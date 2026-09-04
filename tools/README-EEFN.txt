EEFN node for Windows

Run install.ps1. It asks for a dedicated installation directory and whether
EEFN should start when you sign in. Do not treat the download/extraction folder
as the installation directory.

Open http://127.0.0.1:51336/ to configure coordinator endpoints, priorities,
permissions, update policy, and models. EEFN remains a node without a connected
EEF, although coordinated network work requires at least one EEF.

No AI model is included or downloaded. Provider mode auto uses an existing
Ollama service first and otherwise uses the bundled llama.cpp server with an
owner-selected local GGUF file.
