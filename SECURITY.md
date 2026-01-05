# Security Policy

## Supported Versions

We release security updates for the following versions:

| Version | Supported          |
| ------- | ------------------ |
| 0.2.x   | :white_check_mark: |
| < 0.2   | :x:                |

## Reporting a Vulnerability

We take security vulnerabilities seriously. If you discover a security issue, please report it responsibly.

### How to Report

**Do NOT** open a public GitHub issue for security vulnerabilities.

Instead, please email security reports to:

**tindejphachon@gmail.com**

Include the following information in your report:

- Description of the vulnerability
- Steps to reproduce the issue
- Potential impact
- Any suggested fixes (if applicable)
- Your contact information for follow-up

### What to Expect

1. **Acknowledgment**: We will acknowledge receipt of your vulnerability report within 48 hours.

2. **Investigation**: We will investigate the issue and determine its severity and impact.

3. **Updates**: We will keep you informed of our progress. Expect an update at least every 7 days.

4. **Resolution Timeline**:
   - **Critical vulnerabilities**: Patch within 7 days
   - **High severity**: Patch within 14 days
   - **Medium/Low severity**: Patch in next release or within 30 days

5. **Disclosure**: We will coordinate with you on the disclosure timeline. We prefer:
   - Fix is developed and tested
   - Patch is released
   - Public disclosure 7 days after patch release

6. **Credit**: We will credit you in the security advisory (unless you prefer to remain anonymous).

## Security Considerations

### Audio Data

This tool transmits audio data to OpenAI's servers in real-time. Please be aware:

- All audio is sent to OpenAI's Realtime API for transcription
- Audio data handling is subject to [OpenAI's privacy policy](https://openai.com/policies/privacy-policy/)
- Do not use this tool for sensitive, confidential, or regulated audio content unless you've reviewed OpenAI's data handling practices

### API Keys

- Store your `OPENAI_API_KEY` securely
- Never commit API keys to version control
- Use environment variables or secure credential management
- Rotate keys regularly and immediately if compromised

### Local Security

- The daemon listens on a Unix socket (not network-accessible by default)
- Socket permissions are restricted to the user running the daemon
- On Linux: Socket is created in `$XDG_RUNTIME_DIR/dictate/` (tmpfs, user-only)
- On macOS: Socket is created in `~/.local/state/dictate/` (user directory)

## Known Security Limitations

1. **No end-to-end encryption**: Audio is encrypted in transit to OpenAI but processed on their servers
2. **API key in environment**: The OpenAI API key must be available in the daemon's environment
3. **No audio retention controls**: This tool does not control how long OpenAI retains audio data

## Security Best Practices

When using dictate:

1. Only use in environments where you trust the audio content being transcribed
2. Review OpenAI's data retention and privacy policies
3. Use a dedicated OpenAI API key with usage limits set
4. Monitor your API usage for unexpected activity
5. Keep the software up to date with the latest security patches

## Scope

This security policy covers:

- The dictate daemon (`dictated`)
- The CLI tools (`dictate`, `dictatectl`)
- The Neovim plugin
- Build and distribution infrastructure

This policy does **not** cover:

- OpenAI's API infrastructure or data handling
- Third-party dependencies (report to their respective projects)
- Issues with your OpenAI account or API key management

## Contact

For security-related questions or concerns:

- Email: tindejphachon@gmail.com
- General issues (non-security): [GitHub Issues](https://github.com/tindotdev/dictate/issues)
