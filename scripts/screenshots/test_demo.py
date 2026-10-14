#!/usr/bin/env python3
"""Checks the settings demo.py writes, without a display or a server:

    python3 -m unittest discover -s scripts/screenshots -p 'test_*.py'
"""
import os, sys, unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import demo  # noqa: E402


class Settings(unittest.TestCase):
    def test_previews_avatars_and_compact_urls_are_on_in_appearance(self):
        for language, script in demo.SCRIPTS.items():
            appearance = demo.settings(script, 1234, language, "light")["appearance"]
            for key in ("image_previews", "user_avatars", "compact_urls"):
                self.assertIs(appearance.get(key), True, f"{language}: {key}")

    def test_every_channel_is_joined_and_server_is_local(self):
        for language, script in demo.SCRIPTS.items():
            server = demo.settings(script, 1234, language, "dark")["servers"][0]
            self.assertEqual(server["host"], "127.0.0.1")
            self.assertEqual(server["channels"].split(","),
                             [channel["name"] for channel in script["channels"]])

    def test_the_shown_channel_has_a_me_line(self):
        for language, script in demo.SCRIPTS.items():
            lines = script["channels"][0]["lines"]
            self.assertTrue(any(text.startswith("\x01ACTION ") and text.endswith("\x01")
                                for _, text in lines), language)


if __name__ == "__main__":
    unittest.main()
