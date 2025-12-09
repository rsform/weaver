# Privacy Policy

*Last updated: December 2025*

This privacy policy is a placeholder. A proper privacy policy will be added before Weaver moves into beta.

For an explanation of the state of things and the philosophy, please read [this devlog](https://alpha.weaver.sh/did:plc:yfvwmnlztr4dwkb7hwz55r2g/weaver/drafts_privacy).

## Data Collection

Weaver itself does not collect personal data. However:

- **AT Protocol**: When you authenticate and publish content or sync drafts, that data is stored on your AT Protocol Personal Data Server (PDS) according to the PDS operator's privacy policy. AT Protocol data is almost entirely public. All AT Protocol data Weaver creates and manages on your behalf is readable by anyone with the right tools. This is a protocol limitation. Data can be obfuscated or requested to be hidden, but aside from a small subset of Bluesky-specific data it cannot be hidden from public view.
- **Bluesky**: If you use a Bluesky-operated PDS, Bluesky's privacy policy applies to that data.
- **Iroh**: Real-time collaboration currently traverses Iroh's public relays due to browser limitations. The data is encrypted end-to-end with an ephemeral session key and cannot be read by them. Weaver will host its own Iroh relay(s) in the future for production use, with similar guarantees.

## Cookies

Weaver uses local storage to maintain your authentication session and unsynced draft state. No tracking cookies are used.

## Analytics

Weaver currently uses a Cloudflare tunnel to proxy the app server out to the public web and has Cloudflare's analytics enabled, which collects basic location and performance metrics.

## Contact

For privacy concerns, please open an issue on the [project repository](https://tangled.org/nonbinary.computer/weaver/issues) or email contact(at)weaver.sh.
