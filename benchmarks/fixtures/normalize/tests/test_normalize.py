import unittest
from normalize import slugify

class SlugifyTests(unittest.TestCase):
    def test_case(self):
        self.assertEqual(slugify("Agent Execution Engine"),"agent-execution-engine")
    def test_separators(self):
        self.assertEqual(slugify("HELLO__WORLD / 2026"),"hello-world-2026")
    def test_empty(self):
        self.assertEqual(slugify("___..."),"")
