#!/usr/bin/env python3
"""
DSP Protocol Demonstration

"""

import time
import requests
from typing import Dict, Optional


class DSPDemo:
    def __init__(self, server_url: str = "http://localhost:3000"):
        self.server_url = server_url
        self.session_id: Optional[str] = None
        self.resource_versions: Dict[str, str] = {}
        self.stats = {
            "total_requests": 0,
            "full_responses": 0,
            "diff_responses": 0,
            "bytes_received": 0,
            "bytes_if_no_dsp": 0,
            "total_savings": 0,
            "scenarios": {},
        }

    def fetch_resource(self, path: str, scenario: str = "default") -> Dict:
        """Fetch a resource with DSP headers"""
        headers = {}
        if self.session_id:
            headers["X-DSP-Session"] = self.session_id
        if path in self.resource_versions:
            headers["X-Base-Version"] = self.resource_versions[path]
            headers["Accept-Diff"] = "binary-delta"

        response = requests.get(f"{self.server_url}{path}", headers=headers)
        response.raise_for_status()

        # Update session state
        self.session_id = response.headers.get("X-DSP-Session", self.session_id)
        if response.headers.get("X-Resource-Version"):
            self.resource_versions[path] = response.headers["X-Resource-Version"]

        # Track statistics
        self.stats["total_requests"] += 1
        content_length = len(response.content)
        self.stats["bytes_received"] += content_length

        # Track per-scenario stats
        if scenario not in self.stats["scenarios"]:
            self.stats["scenarios"][scenario] = {
                "requests": 0,
                "diffs": 0,
                "bytes_received": 0,
                "bytes_saved": 0,
            }

        scenario_stats = self.stats["scenarios"][scenario]
        scenario_stats["requests"] += 1
        scenario_stats["bytes_received"] += content_length

        result = {
            "content": response.content,
            "diff_type": response.headers.get("X-Diff-Type", "unknown"),
            "content_length": content_length,
            "original_size": response.headers.get("X-Original-Size"),
            "diff_size": response.headers.get("X-Diff-Size"),
            "resource_version": response.headers.get("X-Resource-Version"),
        }

        if result["diff_type"] == "binary-delta":
            self.stats["diff_responses"] += 1
            scenario_stats["diffs"] += 1
            if result["original_size"]:
                original_size = int(result["original_size"])
                self.stats["bytes_if_no_dsp"] += original_size
                savings = original_size - content_length
                self.stats["total_savings"] += savings
                scenario_stats["bytes_saved"] += savings
        else:
            self.stats["full_responses"] += 1
            self.stats["bytes_if_no_dsp"] += content_length

        return result

    def print_scenario_header(self, title: str, description: str):
        print(f"\n{'='*60}")
        print(f"SCENARIO: {title}")
        print(f"{'='*60}")
        print(f"{description}")
        print(f"{'-'*60}")

    def print_result(self, result: Dict, step: str, show_content: bool = False):
        status = "DIFF" if result["diff_type"] == "binary-delta" else "FULL"
        size = result["content_length"]

        if result["diff_type"] == "binary-delta" and result["original_size"]:
            original = int(result["original_size"])
            savings = original - size
            savings_pct = (savings / original) * 100
            print(
                f"{step}: {status} ({size:,} bytes, saved {savings:,} bytes = {savings_pct:.1f}% reduction)"
            )
        else:
            print(f"{step}: {status} ({size:,} bytes)")

        if show_content and result["content_length"] < 1000:
            content_preview = result["content"].decode("utf-8", errors="replace")[:200]
            print(f"   Preview: {content_preview}...")

    def print_final_stats(self):
        """Print comprehensive statistics"""
        print(f"\n{'='*60}")
        print("DSP PROTOCOL DEMONSTRATION")
        print(f"{'='*60}")

        print(f"Overall Statistics:")
        print(f"  Total Requests:       {self.stats['total_requests']}")
        print(f"  Full Responses:       {self.stats['full_responses']}")
        print(f"  Diff Responses:       {self.stats['diff_responses']}")
        print()

        print(f"Bandwidth Analysis:")
        print(f"  Bytes Received (DSP): {self.stats['bytes_received']:,} bytes")
        print(f"  Bytes Without DSP:    {self.stats['bytes_if_no_dsp']:,} bytes")
        print(f"  Total Bytes Saved:    {self.stats['total_savings']:,} bytes")

        if self.stats["bytes_if_no_dsp"] > 0:
            overall_savings = (
                self.stats["total_savings"] / self.stats["bytes_if_no_dsp"]
            ) * 100
            print(f"  Overall Savings:      {overall_savings:.1f}%")

        print(f"\nPer-Scenario Breakdown:")
        for scenario, stats in self.stats["scenarios"].items():
            if stats["requests"] > 0:
                diff_rate = (stats["diffs"] / stats["requests"]) * 100
                print(
                    f"  {scenario:20s}: {stats['requests']:2d} requests, {stats['diffs']:2d} diffs ({diff_rate:.0f}%), saved {stats['bytes_saved']:,} bytes"
                )


def main():
    print("DSP PROTOCOL DEMONSTRATION")
    print(f"{'='*60}")

    client = DSPDemo()

    try:
        # SCENARIO 1: Log Stream Monitoring (Append-Only)
        client.print_scenario_header(
            "Log Stream Monitoring",
            "Simulating real-time log monitoring where new entries are appended.\n",
        )

        # Initial fetch
        result = client.fetch_resource("/api/logs/server", "log_monitoring")
        client.print_result(result, "Initial log fetch")

        # Simulate log monitoring with new entries
        for i in range(5):
            print(f"\nWaiting 2 seconds for new log entries...")
            time.sleep(2)

            # Trigger log update
            requests.get(f"{client.server_url}/demo/update")

            # Fetch updated logs
            result = client.fetch_resource("/api/logs/server", "log_monitoring")
            client.print_result(result, f"Log fetch #{i+1} (new entries)")

        # SCENARIO 2: Metrics Dashboard (Incremental Updates)
        client.print_scenario_header(
            "Live Metrics Dashboard",
            "Simulating a monitoring dashboard where numeric values change incrementally.\n",
        )

        # Initial dashboard fetch
        result = client.fetch_resource("/api/dashboard/metrics", "metrics_dashboard")
        client.print_result(result, "Initial metrics fetch")

        # Simulate dashboard polling
        for i in range(4):
            print(f"\nPolling dashboard metrics (iteration {i+1})...")
            time.sleep(1)

            # Trigger metrics update
            requests.get(f"{client.server_url}/demo/update")

            # Fetch updated metrics
            result = client.fetch_resource(
                "/api/dashboard/metrics", "metrics_dashboard"
            )
            client.print_result(result, f"Metrics update #{i+1}")

        # SCENARIO 3: Collaborative Document (Text Editing)
        client.print_scenario_header(
            "Collaborative Document Editing",
            "Simulating real-time collaborative editing where small text changes are made.\n",
        )

        # Initial document fetch
        result = client.fetch_resource(
            "/api/documents/collaborative", "collaborative_editing"
        )
        client.print_result(result, "Initial document fetch", show_content=True)

        # Simulate collaborative editing
        for i in range(3):
            print(f"\nSimulating document edit #{i+1}...")
            time.sleep(1)

            # Trigger document update
            requests.get(f"{client.server_url}/demo/update")

            # Fetch updated document
            result = client.fetch_resource(
                "/api/documents/collaborative", "collaborative_editing"
            )
            client.print_result(result, f"Document after edit #{i+1}")

        client.print_final_stats()

    except requests.exceptions.ConnectionError:
        print("\nError: Could not connect to DSP server!")
        print("Start the server with: cargo run --example server")
    except Exception as e:
        print(f"\nError: {e}")


if __name__ == "__main__":
    main()
