const http = require("http");
const server = http.createServer((req, res) => {
    res.end("broken-project running");
});
server.listen(0, () => {
    const addr = server.address();
    console.log(`http://localhost:${addr.port}`);
});
