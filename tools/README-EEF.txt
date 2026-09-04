EEF coordinator for Windows

Run install.ps1. It asks for a dedicated installation directory and whether
EEF should start when you sign in. Do not treat the download/extraction folder
as the installation directory.

EEF is a coordinator, not a node. It waits for at least one EEFN node before it
becomes operational. Set EEF_NODE_PSK to the shared network secret and open
http://127.0.0.1:51334/ for the dashboard.

No AI model is included. EEF routes model work only to owner-selected models on
connected nodes. Update policy defaults to prompt and is configurable.
