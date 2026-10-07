# printobserver-server

## Every request is the identity its credential authenticated

The server keeps only the SHA-256 verifier of the operator's credential
(`api.credential_verifier`, else `<state_dir>/api-credential.verifier`) and
never a plaintext one it wrote: `server::take_plaintext_out` converts and
deletes the legacy files at start. Each supervision turn is minted a credential
of its own by `turns::TurnCredentials`, held in memory, handed to the turn in
its environment alone, bound to the agent, that turn's session and its print,
and revoked when the turn returns.

`api::Admission` resolves the caller before any handler runs, and each handler
refuses `403` — before the body is parsed or the policy asked — a claim to
another actor or print, any claim to `system`, and a turn starting a print or
replacing a manifest. `tests/journeys/binding.rs` drives each refusal.
