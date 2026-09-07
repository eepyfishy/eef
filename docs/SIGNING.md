# Windows release signing

Public signing is not configured. No signing certificate/private key is present
in the current developer account, and no identity has been enrolled or sent to
a provider. Do not publish development files as signed or antivirus-cleared.

Signing and malware review are separate. Microsoft's
[SmartScreen guidance](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)
explains publisher reputation; new signed applications can still receive warnings.
The reported Sabsik/Wacatac alerts are antivirus detections, not merely unknown
publisher warnings. Submit those exact samples for a separate Defender verdict.

## Privacy and provider choice

Do not embed personal identity in the project or use another project's signing
account. Microsoft's [Artifact Signing FAQ](https://learn.microsoft.com/en-us/azure/artifact-signing/faq)
requires validated legal identity for public certificate subject fields; an
anonymous GitHub alias is not a substitute. Public certificate details are visible.
[SignPath Foundation](https://signpath.org/) offers an open-source program subject
to eligibility and review; approval, onboarding privacy, and compatibility with
this project's dependencies must be checked before choosing it.

## Signing order and verification

1. Build reviewed first-party source with locked, verified dependencies.
2. Sign `eef.exe` and `eefn.exe` before creating bundle inventories and payloads.
3. Append payloads to the **unsigned** installer stub, then sign each final
   installer. Never append a payload after signing the stub.
4. Verify the complete Authenticode signature and timestamp with SignTool's
   Authenticode policy, scan final artifacts, and hash the final signed bytes.
5. Verify the two uploaded release assets match those final hashes.

Use an explicitly selected signing credential and SHA-256/RFC3161 timestamping.
`tools/bundle-windows.ps1` accepts `-RequireSigning`, `-SignToolPath`,
`-CertificateThumbprint`, and `-TimestampUrl`. All three credential/tool/service
parameters must be supplied together; required signing without them fails before
the build. `tools/sign-windows.ps1` uses only the selected CurrentUser certificate,
requires a private key and code-signing EKU, and verifies signer, trust and timestamp
after signing. These hooks have not been exercised with a real signing credential.

Do not create a self-signed trusted root, change system trust, re-sign vendor
DLLs as our own, store passwords in source, or disable scan gates.

The shared payload reader parses terminal PE certificate table framing including
0–7 alignment bytes. It does **not** verify certificate trust or signer identity.
Tests use synthetic certificate framing, not real trusted signatures. See the
[PE format specification](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format)
and [SignTool reference](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool).

## Upgrade compatibility

The released v0.3.2 updater expects the footer at physical EOF and cannot unpack
an Authenticode-signed installer. The first signed release needs a manual
installer upgrade or a separately tested bridge. Do not change existing update
feeds to signed installers and assume old clients can install them. The
development reader remains compatible with existing unsigned installer payloads.
