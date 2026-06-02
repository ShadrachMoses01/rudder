package main

import (
    "fmt"
    "net/http"
)

func main() {
    http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
        fmt.Fprintln(w, "go-app running")
    })
    fmt.Println("http://localhost:8081")
    http.ListenAndServe(":8081", nil)
}
