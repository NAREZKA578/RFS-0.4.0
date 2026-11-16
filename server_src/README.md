# server_src — выделенный сервер

The dedicated server: the `rfs_server` binary and the crates it is built
from. Build and run from this directory:

    cargo run

## Authentication dialect

The shipped `server_config.json` runs the **unsigned** dialect
(`allow_unsigned: true`, blank token): every datagram is accepted on
framing alone. A fresh clone starts and connects with no setup — the
client needs neither a config nor a token (`RFS_TOKEN` unset).

The **keyed** dialect verifies an HMAC tag on every datagram (wire auth,
bug №700). To run it:

1. Generate your own 32-byte secret (64 hex characters) — for example
   `openssl rand -hex 32`, or in PowerShell:
   `$b = New-Object byte[] 32; [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($b); -join ($b | ForEach-Object { $_.ToString('x2') })`
2. Copy `server_config.example.json` to `server_config.local.json`
   (gitignored) and put the secret into `auth_token`.
3. Start the server with that file:
   `cargo run -- --config server_config.local.json`
4. Give every client the same secret through the `RFS_TOKEN`
   environment variable.

Rotating the wire secret: write the new one into
`server_config.local.json` and restart the server. Never commit the
secret: the shipped config stays unsigned, and operator secrets live
only in gitignored files.

## Control channel

The control HTTP port has its own token: `control_token.json` is
generated on first start, honoured (not regenerated) while the file
exists, and is never committed. Rotating it: stop the server, delete
the file, start again — a fresh token is written.
