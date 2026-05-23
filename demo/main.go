package main

import (
	"flag"
	"fmt"
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

	app.Listen(fmt.Sprintf(":%d", *port))
}
