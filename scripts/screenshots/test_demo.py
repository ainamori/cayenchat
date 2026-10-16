#!/usr/bin/env python3
"""Checks the settings demo.py writes, without a display or a server:

    python3 -m unittest discover -s scripts/screenshots -p 'test_*.py'
"""
import os, sys, tempfile, time, unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import demo  # noqa: E402


class Settings(unittest.TestCase):
    def test_previews_and_compact_urls_are_on_and_avatars_on_for_the_server(self):
        for language, script in demo.SCRIPTS.items():
            settings = demo.settings(script, 1234, language, "light")
            for key in ("image_previews", "compact_urls"):
                self.assertIs(settings["appearance"].get(key), True, f"{language}: {key}")
            self.assertIs(settings["servers"][0].get("user_avatars"), True, language)

    def test_the_toolbar_is_shown_unless_asked_otherwise(self):
        for language, script in demo.SCRIPTS.items():
            self.assertIs(demo.settings(script, 1234, language, "light")["show_toolbar"], True)
            self.assertIs(demo.settings(script, 1234, language, "light", False)["show_toolbar"],
                          False)

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


class Build(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.path = os.path.join(self.scratch.name, "cayenchat")

    def tearDown(self):
        self.scratch.cleanup()

    def write(self, markers):
        with open(self.path, "wb") as file:
            file.write(b"\0".join(markers))

    def test_a_missing_binary_is_refused(self):
        self.assertTrue(demo.build_problems(self.path))

    def test_a_build_without_the_demo_feature_is_refused(self):
        self.write([])
        os.utime(self.path, (time.time() + 3600,) * 2)
        self.assertTrue(any("demo" in p for p in demo.build_problems(self.path)))

    def test_a_build_older_than_the_sources_is_refused(self):
        self.write(demo.DEMO_MARKERS)
        os.utime(self.path, (1, 1))
        self.assertTrue(any("older" in p for p in demo.build_problems(self.path)))

    def test_a_fresh_demo_build_is_accepted(self):
        self.write(demo.DEMO_MARKERS)
        os.utime(self.path, (time.time() + 3600,) * 2)
        self.assertEqual(demo.build_problems(self.path), [])


if __name__ == "__main__":
    unittest.main()
