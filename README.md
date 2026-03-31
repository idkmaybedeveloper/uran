# uran
yet another [cobalt](https://github.com/imputnet/cobalt/tree/main/api) compatible api, now in rust.

### currently implemented:
- [x] twitter (graphql + syndication fallback)
- [x] tiktok (universal data parsing + rehydration)
- [ ] youtube??
- [ ] BlueSky??

### how to run:
```bash
cargo run --release
```

### config:
- `PORT`: server port (default: `8080`)
- `USER_AGENT`: custom user agent for requests
