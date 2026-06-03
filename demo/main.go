package main

import (
	"context"
	"fmt"
	"io"
	"net/http"
	"os"

	"github.com/gofiber/fiber/v3"
	"github.com/jackc/pgx/v5"
)

func main() {
	// port := flag.Int("port", 3000, "port to listen on")
	// flag.Parse()

	port := os.Getenv("PORT")
	if port == "" {
		fmt.Println("No env PORT set. defaulting to 3000")
		port = "3000"
	}

	app := fiber.New()

	app.Get("/", func(c fiber.Ctx) error {
		app_name := os.Getenv("APP_NAME")
		if app_name == "" {
			app_name = "World"
		}
		return c.SendString("Hello, " + app_name + "!")
	})

	app.Get("/health", func(c fiber.Ctx) error {
		return c.Status(200).SendString("OK!")
	})

	app.Get("/pg", pgTestHandler)

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

		return c.Status(resp.StatusCode).SendString(
			fmt.Sprintf("Response from %s: %s", to, string(body)),
		)
	})

	app.Get("/counter", func(c fiber.Ctx) error {
		leader := os.Getenv("POSTGRES_MYDB_LEADER")
		follower := os.Getenv("POSTGRES_MYDB_FOLLOWER")
		if leader == "" {
			return c.Status(fiber.StatusInternalServerError).SendString("missing POSTGRES_MYDB_LEADER env")
		}
		if follower == "" {
			follower = leader
		}

		ctx := context.Background()
		if err := ensureCounterTableExists(ctx, leader); err != nil {
			return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf("Failed to ensure counter table: %v", err))
		}

		conn, err := pgx.Connect(ctx, follower)
		if err != nil {
			return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf("Failed to connect to FOLLOWER: %v", err))
		}
		defer conn.Close(ctx)

		var val int
		err = conn.QueryRow(ctx, "SELECT value FROM api_counter WHERE id = 1").Scan(&val)
		if err != nil {
			return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf("Failed to query counter: %v", err))
		}

		return c.JSON(fiber.Map{"count": val})
	})

	app.Post("/counter/add", addCounterHandler)
	app.Get("/counter/add", addCounterHandler)
	app.Post("/counter", addCounterHandler)

	app.Listen(fmt.Sprintf(":%s", port))
}

func addCounterHandler(c fiber.Ctx) error {
	leader := os.Getenv("POSTGRES_MYDB_LEADER")
	if leader == "" {
		return c.Status(fiber.StatusInternalServerError).SendString("missing POSTGRES_MYDB_LEADER env")
	}

	ctx := context.Background()
	if err := ensureCounterTableExists(ctx, leader); err != nil {
		return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf("Failed to ensure counter table: %v", err))
	}

	conn, err := pgx.Connect(ctx, leader)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf("Failed to connect to LEADER: %v", err))
	}
	defer conn.Close(ctx)

	var val int
	err = conn.QueryRow(ctx, "UPDATE api_counter SET value = value + 1 WHERE id = 1 RETURNING value").Scan(&val)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf("Failed to increment counter: %v", err))
	}

	return c.JSON(fiber.Map{"count": val})
}

func ensureCounterTableExists(ctx context.Context, connStr string) error {
	conn, err := pgx.Connect(ctx, connStr)
	if err != nil {
		return err
	}
	defer conn.Close(ctx)

	_, err = conn.Exec(ctx, "CREATE TABLE IF NOT EXISTS api_counter (id INT PRIMARY KEY, value INT NOT NULL)")
	if err != nil {
		return err
	}

	_, err = conn.Exec(ctx, "INSERT INTO api_counter (id, value) VALUES (1, 0) ON CONFLICT (id) DO NOTHING")
	return err
}

func pgTestHandler(c fiber.Ctx) error {
	leader_uri := os.Getenv("POSTGRES_MYDB_LEADER")
	if leader_uri == "" {
		leader_uri = os.Getenv("POSTGRES_MYDB_LEADER")
	}

	follower_uri := os.Getenv("POSTGRES_MYDB_FOLLOWER")
	if follower_uri == "" {
		follower_uri = os.Getenv("POSTGRES_MYDB_FOLLOWER")
	}

	if leader_uri == "" || follower_uri == "" {
		return c.Status(fiber.StatusInternalServerError).SendString(fmt.Sprintf(
			"missing env variables: LEADER=%q, FOLLOWER=%q (also checked MYDB uppercase)", leader_uri, follower_uri,
		))
	}

	single_node_deploy := leader_uri == follower_uri

	ctx := context.Background()

	leaderConnStatus := "skipped"
	tableCreateStatus := "skipped"
	insertStatus := "skipped"
	updateStatus := "skipped"
	selectLeaderStatus := "skipped"
	replicaConnStatus := "skipped"
	readReplicaStatus := "skipped"
	writeReplicaStatus := "skipped"
	tableDropStatus := "skipped"

	var insertedID int
	var leaderVal string
	var followerVal string

	hasCreatedTable := false
	hasInserted := false

	// 1. Connect to Leader
	leaderConn, err := pgx.Connect(ctx, leader_uri)
	if err != nil {
		leaderConnStatus = fmt.Sprintf("failed to connect: %v", err)
	} else {
		leaderConnStatus = "success ✅"
		defer leaderConn.Close(ctx)

		// 2. Create Table on Leader
		_, err = leaderConn.Exec(ctx, "CREATE TABLE IF NOT EXISTS test_replication (id serial PRIMARY KEY, val text)")
		if err != nil {
			tableCreateStatus = fmt.Sprintf("failed: %v", err)
		} else {
			tableCreateStatus = "success ✅"
			hasCreatedTable = true

			// Ensure cleanup even if we fail later
			defer func() {
				cleanupCtx := context.Background()
				cleanupConn, cleanupErr := pgx.Connect(cleanupCtx, leader_uri)
				if cleanupErr == nil {
					cleanupConn.Exec(cleanupCtx, "DROP TABLE IF EXISTS test_replication")
					cleanupConn.Close(cleanupCtx)
				}
			}()

			// 3. Run CRUD on Leader: Insert
			err = leaderConn.QueryRow(ctx, "INSERT INTO test_replication (val) VALUES ($1) RETURNING id", "hello_replication").Scan(&insertedID)
			if err != nil {
				insertStatus = fmt.Sprintf("failed: %v", err)
			} else {
				insertStatus = fmt.Sprintf("success ✅ (id=%d)", insertedID)
				hasInserted = true

				// 4. Run CRUD on Leader: Update
				_, err = leaderConn.Exec(ctx, "UPDATE test_replication SET val = $1 WHERE id = $2", "updated_replication", insertedID)
				if err != nil {
					updateStatus = fmt.Sprintf("failed: %v", err)
				} else {
					updateStatus = "success ✅"

					// 5. Run CRUD on Leader: Select
					err = leaderConn.QueryRow(ctx, "SELECT val FROM test_replication WHERE id = $1", insertedID).Scan(&leaderVal)
					if err != nil {
						selectLeaderStatus = fmt.Sprintf("failed: %v", err)
					} else if leaderVal != "updated_replication" {
						selectLeaderStatus = fmt.Sprintf("failed (value mismatch: got %q, want %q)", leaderVal, "updated_replication")
					} else {
						selectLeaderStatus = fmt.Sprintf("success ✅ (value=%q)", leaderVal)
					}
				}
			}
		}
	}

	// 6. Connect to Follower
	if leaderConnStatus == "success ✅" {
		followerConn, err := pgx.Connect(ctx, follower_uri)
		if err != nil {
			replicaConnStatus = fmt.Sprintf("failed to connect to %s: %v", follower_uri, err)
		} else {
			replicaConnStatus = "success ✅"
			defer followerConn.Close(ctx)

			// 7. Get data from created table on FOLLOWER to test replication
			if hasInserted {
				err = followerConn.QueryRow(ctx, "SELECT val FROM test_replication WHERE id = $1", insertedID).Scan(&followerVal)
				if err != nil {
					readReplicaStatus = fmt.Sprintf("failed: %v", err)
				} else if followerVal != "updated_replication" {
					readReplicaStatus = fmt.Sprintf("failed (value mismatch: got %q, want %q)", followerVal, "updated_replication")
				} else {
					readReplicaStatus = fmt.Sprintf("success ✅ (value=%q)", followerVal)
				}
			}

			// 8. Try write to POSTGRES_MYDB_FOLLOWER to assert it failed
			if hasCreatedTable {
				_, writeOnReplicaErr := followerConn.Exec(ctx, "INSERT INTO test_replication (val) VALUES ($1)", "should_fail")

				if writeOnReplicaErr == nil {
					if single_node_deploy {
						writeReplicaStatus = "succeeded ✅ (expected success in single-node)"
					} else {
						writeReplicaStatus = "failed ❌ (expected failure for read-only replica)"
					}
				} else {
					if single_node_deploy {
						writeReplicaStatus = fmt.Sprintf("failed ❌ (expected success in single-node): %v", writeOnReplicaErr)
					} else {
						writeReplicaStatus = fmt.Sprintf("failed ✅ (expected failure for read-only replica): %v", writeOnReplicaErr)
					}
				}
			}
		}
	}

	// 9. Delete table
	if leaderConnStatus == "success ✅" && hasCreatedTable {
		_, err = leaderConn.Exec(ctx, "DROP TABLE test_replication")
		if err != nil {
			tableDropStatus = fmt.Sprintf("failed: %v", err)
		} else {
			tableDropStatus = "success ✅"
		}
	}

	// Return success response with details
	resultText := fmt.Sprintf(
		"Postgres Cluster Test Result:\n"+
			"- Leader Connection: %s\n"+
			"- Replica Connection: %s\n"+
			"- Table created on LEADER: %s\n"+
			"- Inserted on LEADER: %s\n"+
			"- Updated on LEADER: %s\n"+
			"- Selected on LEADER: %s\n"+
			"- Read from replica: %s\n"+
			"- Attempted write to replica: %s\n"+
			"- Table dropped on LEADER: %s\n",
		leaderConnStatus, replicaConnStatus, tableCreateStatus, insertStatus, updateStatus, selectLeaderStatus, readReplicaStatus, writeReplicaStatus, tableDropStatus,
	)

	return c.SendString(resultText)
}
