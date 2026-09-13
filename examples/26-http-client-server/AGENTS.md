# std.net example

Run `qz run` for usage instructions. Start `qz run --bin http-server`, then run
`qz run --bin http-client` in another terminal. Server handles one HTTP request,
then exits. `qz run --bin http-url-client` is the `HttpRequest.send_url`
regression client: the server must print `GET /url-routed HTTP/1.1` and
`Host: localhost:8080`, proving the URL replaced its deliberately incorrect
constructor host and target and that hostname resolution succeeds. This works
identically on Windows and Linux without forwarding raw arguments through `qz run`.
