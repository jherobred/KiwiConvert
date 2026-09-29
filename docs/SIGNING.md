# Code signing and SmartScreen

Windows SmartScreen shows "Windows protected your PC" for programs downloaded with a
browser that it hasn't seen often. It looks at two things: whether the program is signed
with a code-signing certificate from a trusted authority, and how many people have run
files signed with that certificate. An unsigned installer keeps showing the warning no
matter how many people download it, because every new release is a new, unknown file.

Signing removes the "Unknown publisher" label right away. The warning itself fades once the
certificate has built reputation, which takes some downloads. There is no switch or file
that skips this for other people's PCs, and a self-signed certificate doesn't help, because
only the PC that trusts it would accept it.

## Ways to get a certificate

| Option | Cost | Notes |
| --- | --- | --- |
| [SignPath Foundation](https://signpath.org) | Free for open-source projects | Apply with the repository. Builds must come from CI. |
| [Azure Trusted Signing](https://azure.microsoft.com/products/trusted-signing) | About $10 a month | Identity check required. Microsoft-issued certificates. |
| A certificate authority (Certum, Sectigo, DigiCert) | About $50 to $400 a year | Certum offers a lower-cost certificate for open-source developers. |

## Signing the build

`scripts/build-installer.ps1` signs the app, the uninstaller and the installer when
`KIWI_SIGN_COMMAND` is set. The command signs one file, with `%1` where the path goes.

With Azure Trusted Signing and [trusted-signing-cli](https://crates.io/crates/trusted-signing-cli):

```powershell
$env:AZURE_CLIENT_ID = "..."
$env:AZURE_CLIENT_SECRET = "..."
$env:AZURE_TENANT_ID = "..."
$env:KIWI_SIGN_COMMAND = 'trusted-signing-cli -e https://eus.codesigning.azure.net -a <account> -c <profile> -d KiwiConvert "%1"'
powershell -File scripts/build-installer.ps1
```

With a certificate file and signtool from the Windows SDK:

```powershell
$env:KIWI_SIGN_COMMAND = 'signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /f C:\secure\cert.pfx /p <password> "%1"'
```

## Signing in the release workflow

`.github/workflows/release.yml` signs with Azure Trusted Signing when these are set in the
repository settings. Without them it publishes unsigned builds.

- Secrets: `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TENANT_ID`
- Variables: `SIGNING_ENDPOINT`, `SIGNING_ACCOUNT`, `SIGNING_PROFILE`

Never commit certificates, passwords or keys. `.gitignore` excludes `*.pfx`, `*.p12`,
`*.pem` and `*.key` files as a safeguard.
