from calc import add


def test_add():
    # The expectation is wrong on purpose: no correct add() can satisfy it.
    assert add(2, 2) == 5
