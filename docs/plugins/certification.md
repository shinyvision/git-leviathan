# Plugin Certification

Run certification before publishing a plugin:

```bash
cargo xtask plugin certify path/to/plugin
```

The suite runs:

- lint for manifest shape, public API calls, capabilities, widgets, slots, and context fields
- load smoke tests using generated host stubs
- render preview of UI contributions
- reload preview to catch duplicate registration or non-idempotent init code
- unload and capability audit checks over manifest declarations

Certification targets plugin API `1.0`, widget schema `1`, and extension-point
schema `1`.
