"""Tests for gaia/assets/robot/robot-env: the settings a board writes before Orion starts.

Orion refuses mdns discovery unless peer auth is required (docs/node-env.md). The script must
never write one without the other, in any case, so every output is checked against that rule.
Run: python3 -m unittest tools/test_robot_env.py
"""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "gaia" / "assets" / "robot" / "robot-env"


def run(kind, team=None, key=False, team_is_json=False, secured=False):
    """Runs robot-env against temp inputs and returns the env it wrote as a dict.

    `secured` writes HeliOS's auth file (device security on); without it the device is open.
    """
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        out = root / "robot.env"
        team_file = root / ("controller.json" if team_is_json else "team.txt")
        if team is not None:
            team_file.write_text('{"team": %d}\n' % team if team_is_json else "%d\n" % team)
        key_file = root / "robot-enrollment.key"
        if key:
            key_file.write_text("not-a-real-key\n")
        auth_file = root / "auth.json"
        if secured:
            auth_file.write_text('{"password_hash": "not-a-real-hash"}\n')
        env = dict(
            os.environ,
            HELIOS_ROBOT_ENV=str(out),
            HELIOS_ROBOT_TEAM_FILES=f"{root}/missing:{team_file}",
            HELIOS_ROBOT_KEY_FILES=f"{root}/missing:{key_file}",
            HELIOS_ROBOT_AUTH_FILE=str(auth_file),
        )
        subprocess.run(["sh", str(SCRIPT), kind], env=env, check=True, capture_output=True)
        lines = out.read_text().splitlines()
        return dict(line.split("=", 1) for line in lines if "=" in line)


def satisfies_orion(settings):
    """Orion's rule for discovery: mdns needs required peer auth."""
    if settings.get("ORION_NODE_DISCOVERY") == "mdns":
        return settings.get("ORION_NODE_PEER_AUTH") == "required"
    return True


class RobotEnvTest(unittest.TestCase):
    def test_a_secured_robot_discovers_with_required_auth_and_its_key(self):
        settings = run("raze", team=6390, key=True)
        self.assertEqual(settings["ORION_NODE_DISCOVERY"], "mdns")
        self.assertEqual(settings["ORION_NODE_PEER_AUTH"], "required")
        self.assertIn("ORION_NODE_ENROLLMENT_KEY_FILE", settings)
        self.assertEqual(settings["ORION_NODE_CLUSTER"], "robot-6390")
        self.assertNotIn("ORION_NODE_ROBOT_LINKS", settings, "a key-enrolled robot does not trust by link")

    def test_an_open_robot_leaves_discovery_to_helios(self):
        settings = run("systemcore", team=6390, key=False)
        self.assertEqual(settings["ORION_NODE_DISCOVERY"], "off")
        self.assertNotIn("ORION_NODE_ENROLLMENT_KEY_FILE", settings)
        self.assertEqual(settings["HELIOS_API_PEER_NETS"], "172.31.12.80/29,10.63.90.0/24")

    def test_an_open_robot_trusts_peers_on_its_own_links(self):
        settings = run("systemcore", team=6390, key=False)
        self.assertEqual(settings["ORION_NODE_PEER_AUTH"], "disabled")
        self.assertEqual(settings["ORION_NODE_PEER_AUTO_TRUST"], "robot-link")
        self.assertEqual(settings["ORION_NODE_ROBOT_LINKS"], "172.31.0.0/16,172.31.12.80/29,10.63.90.0/24")

    def test_a_secured_helios_keeps_orions_own_rules(self):
        settings = run("raze", team=6390, key=False, secured=True)
        self.assertNotIn("ORION_NODE_PEER_AUTH", settings, "the base file's required applies")
        self.assertNotIn("ORION_NODE_ROBOT_LINKS", settings)
        self.assertNotIn("ORION_NODE_PEER_AUTO_TRUST", settings)
        self.assertEqual(settings["ORION_NODE_DISCOVERY"], "off")

    def test_no_team_means_no_robot(self):
        settings = run("raze", team=None, key=True)
        self.assertEqual(settings["ORION_NODE_DISCOVERY"], "off")
        self.assertNotIn("ORION_NODE_CLUSTER", settings)
        self.assertNotIn("ORION_NODE_ROBOT_LINKS", settings)
        self.assertEqual(settings["HELIOS_API_PEER_NETS"], "")

    def test_the_team_can_come_from_a_controller_document(self):
        settings = run("raze", team=254, key=False, team_is_json=True)
        self.assertEqual(settings["ORION_NODE_CLUSTER"], "robot-254")

    def test_every_case_satisfies_orion(self):
        for kind in ("raze", "systemcore"):
            for team in (None, 6390):
                for key in (False, True):
                    for team_is_json in (False, True):
                        with self.subTest(kind=kind, team=team, key=key, json=team_is_json):
                            self.assertTrue(satisfies_orion(run(kind, team=team, key=key, team_is_json=team_is_json)))

    def test_the_node_id_is_unique_per_board_kind(self):
        self.assertTrue(run("raze")["ORION_NODE_ID"].startswith("raze-"))
        self.assertTrue(run("systemcore")["HELIOS_NODE_ID"].startswith("systemcore-"))


if __name__ == "__main__":
    unittest.main()
