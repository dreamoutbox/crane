package main

import (
	"flag"
	"fmt"
	"io"
	"net/http"

	"github.com/gofiber/fiber/v3"
)

func main() {
	port := flag.Int("port", 3000, "port to listen on")
	flag.Parse()

	app := fiber.New()

	app.Get("/", func(c fiber.Ctx) error {
		return c.SendString("Hello, World!")
	})

	app.Get("/health", func(c fiber.Ctx) error {
		return c.SendString("OK!")
	})

	// /curl?to=<service-name> — proxies an HTTP GET to http://<service-name>
	// Used to test cross-service connectivity via /etc/hosts entries.
	app.Get("/curl", func(c fiber.Ctx) error {
		to := c.Query("to")
		if to == "" {
			return c.Status(fiber.StatusBadRequest).SendString("missing ?to= query param")
		}
		target := fmt.Sprintf("http://%s", to)
		resp, err := http.Get(target)
		if err != nil {
			return c.Status(fiber.StatusBadGateway).SendString(fmt.Sprintf("error reaching %s: %v", target, err))
		}
		defer resp.Body.Close()
		body, err := io.ReadAll(resp.Body)
		if err != nil {
			return c.Status(fiber.StatusInternalServerError).SendString("failed to read response body")
		}
		c.Set("Content-Type", resp.Header.Get("Content-Type"))
		return c.Status(resp.StatusCode).Send(body)
	})

	app.Listen(fmt.Sprintf(":%d", *port))
}
