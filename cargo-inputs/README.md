# Original locked build inputs

The capsule preserves all26 original locked packages (25 registry archives and
one complete Git source tree),981 original files and266,907 bytes of original
notices. SHA256 is in inputs.sha256. Compiler/tool prerequisites are separate.

The original Cargo manifests and lock remain unchanged. METIS-CORE is locked to
78ae34090e043e79a206f2daffaa3889389b4790 through branch=master. The receiver
supports that exact selector form while verifying the lock identity and original
commit/tree/blob/mode/grant bytes. It never resolves a branch online. Cargo's
offline source replacement preserves branch=master, rather than rewriting the
manifest or pretending the locked source was declared with rev=. Other branch
selectors remain unsupported pending an explicit source-policy review.

Use new inputs/home folders outside source and compiled target; the receiver
requires a trusted external capsule SHA. Readback rejects changed source or
configuration. Application and original dependency licenses retain their scopes.
