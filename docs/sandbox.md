## Sandbox & approvals

For information about Codex sandboxing and approvals, see [this documentation](https://developers.openai.com/codex/security).

### Windows backend compatibility

This Fork no longer launches commands through the legacy unelevated Windows
backend. CI demonstrated that it could delete files outside permitted writable
roots. Legacy session, capture, wrapper, and managed preflight requests now fail
before changing ACLs or starting a command. They do not silently run without
isolation or automatically switch backends.

Use the existing elevated Windows sandbox backend and complete its dedicated
account setup explicitly. Existing `unelevated` configuration remains readable
but cannot launch sandboxed work. Updating the application does not rewrite that
configuration or provision accounts. The writable-root deletion and protected
metadata regression assertions remain required on the elevated backend.
