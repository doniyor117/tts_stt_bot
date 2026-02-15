import psycopg2
import os

try:
    conn = psycopg2.connect(
        dbname="postgres",
        user="postgres",
        password="password",  # Try default or empty if needed
        host="localhost"
    )
    print("SUCCESS: Connected to Postgres")
    conn.close()
except Exception as e:
    print(f"FAILURE: {e}")
