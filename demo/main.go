package main

import (
	"context"
	"flag"
	"fmt"
	"io"
	"net/http"
	"os"

	"github.com/gofiber/fiber/v3"
	"github.com/jackc/pgx/v5"
)

func main() {
	port := flag.Int("port", 3000, "port to listen on")
	flag.Parse()

	app := fiber.New()

	app.Get("/", func(c fiber.Ctx) error {
		app_name := os.Getenv("APP_NAME")
		if app_name == "" {
			app_name = "World"
		}
		return c.SendString("Hello, " + app_name + "!")
	})

	app.Get("/health", func(c fiber.Ctx) error {
		return c.SendString("OK!")
	})

	app.Get("/pg", func(c fiber.Ctx) error {
		leader := os.Getenv("POSTGRES_MYDB_LEADER")
		if leader == "" {
			leader = os.Getenv("POSTGRES_MYDB_LEADER")
		}

		follower := os.Getenv("POSTGRES_MYDB_FOLLOWER")
		if follower == "" {
			follower = os.Getenv("POSTGRES_MYDB_FOLLOWER")
		}

		if leader == "" || follower == "" {
			return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf(
				"missing env variables: LEADER=%q, FOLLOWER=%q (also checked MYDB uppercase)", leader, follower,
			))
		}

		ctx := context.Background()

		// 1. Connect to Leader
		leaderConn, err := pgx.Connect(ctx, leader)
		if err != nil {
			return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf("Failed to connect to LEADER: %v", err))
		}
		defer leaderConn.Close(ctx)

		// 2. Create Table on Leader
		_, err = leaderConn.Exec(ctx, "CREATE TABLE IF NOT EXISTS test_replication (id serial PRIMARY KEY, val text)")
		if err != nil {
			return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf("Failed to create table on LEADER: %v", err))
		}
		// Ensure cleanup even if we fail later
		defer func() {
			cleanupCtx := context.Background()
			cleanupConn, cleanupErr := pgx.Connect(cleanupCtx, leader)
			if cleanupErr == nil {
				cleanupConn.Exec(cleanupCtx, "DROP TABLE IF EXISTS test_replication")
				cleanupConn.Close(cleanupCtx)
			}
		}()

		// 3. Run CRUD on Leader: Insert
		var insertedID int
		err = leaderConn.QueryRow(ctx, "INSERT INTO test_replication (val) VALUES ($1) RETURNING id", "hello_replication").Scan(&insertedID)
		if err != nil {
			return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf("Failed to insert on LEADER: %v", err))
		}

		// 4. Run CRUD on Leader: Update
		_, err = leaderConn.Exec(ctx, "UPDATE test_replication SET val = $1 WHERE id = $2", "updated_replication", insertedID)
		if err != nil {
			return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf("Failed to update on LEADER: %v", err))
		}

		// 5. Run CRUD on Leader: Select
		var leaderVal string
		err = leaderConn.QueryRow(ctx, "SELECT val FROM test_replication WHERE id = $1", insertedID).Scan(&leaderVal)
		if err != nil {
			return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf("Failed to select on LEADER: %v", err))
		}
		if leaderVal != "updated_replication" {
			return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf("Leader value mismatch: got %q, want %q", leaderVal, "updated_replication"))
		}

		// 6. Connect to Follower
		followerConn, err := pgx.Connect(ctx, follower)
		if err != nil {
			return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf("Failed to connect to FOLLOWER: %v", err))
		}
		defer followerConn.Close(ctx)

		// 7. Get data from created table on FOLLOWER to test replication
		var followerVal string
		err = followerConn.QueryRow(ctx, "SELECT val FROM test_replication WHERE id = $1", insertedID).Scan(&followerVal)
		if err != nil {
			return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf("Failed to select from FOLLOWER: %v", err))
		}
		if followerVal != "updated_replication" {
			return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf("Follower value mismatch: got %q, want %q", followerVal, "updated_replication"))
		}

		// 8. Try write to POSTGRES_MYDB_FOLLOWER to assert it failed
		_, writeErr := followerConn.Exec(ctx, "INSERT INTO test_replication (val) VALUES ($1)", "should_fail")
		if writeErr == nil {
			return c.Status(fiber.StatusInternalServerError).SendString("Assertion failed: write to FOLLOWER succeeded but should have failed (read-only replica)")
		}

		// 9. Delete table
		_, err = leaderConn.Exec(ctx, "DROP TABLE test_replication")
		if err != nil {
			return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf("Failed to drop table on LEADER: %v", err))
		}

		// Return success response with details
		resultText := fmt.Sprintf(
			"Postgres Replication Test Successful!\n"+
				"- Connected to leader: %s\n"+
				"- Connected to follower: %s\n"+
				"- Table created on LEADER\n"+
				"- Inserted and updated row (id=%d) on LEADER\n"+
				"- Read from FOLLOWER: got expected value %q (replication verified)\n"+
				"- Attempted write to FOLLOWER: failed as expected with error: %v\n"+
				"- Table dropped on LEADER\n",
			os.Getenv("POSTGRES_MYDB_LEADER"), os.Getenv("POSTGRES_MYDB_FOLLOWER"), insertedID, followerVal, writeErr,
		)

		return c.SendString(resultText)
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
