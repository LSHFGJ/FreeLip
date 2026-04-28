from freelip_eval.usability_rules import check_top5_usability, is_usable

def test_is_usable_exact_match():
    assert is_usable("帮我写个请假条", "帮我写个请假条", [])

def test_is_usable_equivalent_match():
    assert is_usable("给我写个请假条", "帮我写个请假条", ["给我写个请假条"])

def test_is_usable_punctuation_ignore():
    assert is_usable("帮我写个请假条。", "帮我写个请假条", [])
    assert is_usable("帮我写个请假条", "帮我写个请假条！", [])

def test_is_usable_whitespace_ignore():
    assert is_usable("帮 我 写 个 请假条", "帮我写个请假条", [])

def test_top5_rule_pass():
    candidates = ["错误1", "错误2", "帮我写个请假条", "错误4", "错误5", "错误6"]
    assert check_top5_usability(candidates, "帮我写个请假条", [])

def test_top5_rule_fail():
    candidates = ["错误1", "错误2", "错误3", "错误4", "错误5", "帮我写个请假条"]
    assert not check_top5_usability(candidates, "帮我写个请假条", [])

def test_meaning_change_fail():
    assert not is_usable("帮我写个假条", "帮我定个闹钟", [])
