#!/usr/bin/env python3
from __future__ import annotations

import json
import os
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
HARNESS_PATH = ROOT / ".agent-cube/harness.json"
HARNESS_SCRIPTS = ROOT / "scripts/agent-cube"


def reject_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_harness() -> dict[str, object]:
    return json.loads(
        HARNESS_PATH.read_text(encoding="utf-8"),
        object_pairs_hook=reject_duplicate_keys,
    )


class HarnessRegistryTest(unittest.TestCase):
    def test_every_action_has_one_bounded_executable(self) -> None:
        harness = load_harness()
        self.assertEqual(harness.get("schemaVersion"), 1)
        self.assertEqual(harness.get("project"), "code-mechanic")
        actions = harness.get("actions")
        self.assertIsInstance(actions, dict)
        assert isinstance(actions, dict)
        self.assertTrue(actions)

        commands: list[str] = []
        for action_name, raw_action in sorted(actions.items()):
            with self.subTest(action=action_name):
                self.assertIsInstance(raw_action, dict)
                assert isinstance(raw_action, dict)
                command = raw_action.get("command")
                timeout = raw_action.get("timeoutSeconds")
                self.assertIsInstance(command, str)
                assert isinstance(command, str)
                self.assertTrue(command.startswith("scripts/agent-cube/"))
                self.assertNotIn(" ", command)
                command_path = ROOT / command
                self.assertTrue(command_path.is_file(), f"missing command: {command}")
                self.assertTrue(os.access(command_path, os.X_OK), f"not executable: {command}")
                self.assertIsInstance(timeout, int)
                self.assertNotIsInstance(timeout, bool)
                assert isinstance(timeout, int)
                self.assertGreater(timeout, 0)
                commands.append(command)

        self.assertEqual(len(commands), len(set(commands)))

    def test_every_executable_harness_script_is_registered(self) -> None:
        actions = load_harness()["actions"]
        assert isinstance(actions, dict)
        registered = {
            raw_action["command"]
            for raw_action in actions.values()
            if isinstance(raw_action, dict) and isinstance(raw_action.get("command"), str)
        }
        executables = {
            path.relative_to(ROOT).as_posix()
            for path in HARNESS_SCRIPTS.iterdir()
            if path.is_file() and os.access(path, os.X_OK)
        }
        self.assertEqual(executables, registered)


if __name__ == "__main__":
    unittest.main()
