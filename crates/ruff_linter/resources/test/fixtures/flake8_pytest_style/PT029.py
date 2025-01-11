import unittest
import pytest


class Test(unittest.TestCase):
    def test_foo():
        with pytest.warns():
            do_something()

    def test_bar():
        with pytest.warns(SomeWarning):
            do_something()