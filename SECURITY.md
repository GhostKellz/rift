# Security Policy

## Reporting a Vulnerability

**Please do NOT report security vulnerabilities through public issues or pull requests.**

If you believe you have found a security vulnerability in Rift, please report it through GitHub Security Advisories.

### How to Report

[Report a vulnerability via GitHub Security Advisories](https://github.com/ghostkellz/rift/security/advisories/new)

### What to Include

- Description of the vulnerability
- Steps to reproduce the issue
- Potential impact assessment
- Any suggested remediation (optional)

### Response Timeline

| Stage | Timeframe |
|-------|-----------|
| Initial acknowledgement | 72 hours |
| Status update | 7 days |
| Fix timeline communicated | 14 days |

## Supported Versions

Rift follows a rolling release model. Security patches are applied to the latest version only.

| Version | Supported |
|---------|-----------|
| Latest  | Yes |
| Older   | No  |

Ensure you are running the latest version to receive security updates.

## Security Practices

### IPC Surface
- The daemon listens on a user-scoped Unix domain socket, not a network port
- Socket path is confined to the user runtime directory (`$XDG_RUNTIME_DIR`) with owner-only permissions
- No remote control surface is exposed; `riftctl` and `rift-kwin` connect over the local socket only
- Protocol messages are validated and length-bounded before deserialization

### Privilege Model
- `riftd` runs entirely as the unprivileged user session; it requires no elevation
- The daemon holds no setuid capability and writes only within user-owned config and runtime paths
- The KWin script runs inside the compositor sandbox and forwards events without granting the daemon compositor privileges

### Configuration & State
- Configuration is read from `~/.config/riftrc`; values are parsed and validated on load
- Layout state is derived from live KWin topology and never trusted from persisted input
- Malformed config is rejected with a diagnostic rather than applied partially

### Dependencies
- Regular dependency updates across the Cargo workspace
- `cargo audit` for advisory scanning
- Lockfile-pinned builds for reproducibility

## Disclosure Policy

When a security issue is reported:

1. We confirm receipt and assess severity
2. We develop and test a fix
3. We release the patch
4. We publicly disclose the issue with credit to the reporter (if desired)

We appreciate responsible disclosure and will acknowledge reporters who follow this policy.
