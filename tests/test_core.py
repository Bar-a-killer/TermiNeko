"""neko-core（終端機版和 Claude Code 外掛用的操作）和分幾口吃的規則。"""

import json
import subprocess
import sys

import pytest

from neko_api import cli, core
from neko_api.model import FOODS, KCAL_PER_KG, Cat, need
from neko_api.store import Store

NOW = 1_800_000_000.0


@pytest.fixture
def store(tmp_path):
    return Store(tmp_path / "state.json")


# ───────── 模型：碗和分幾口吃 ─────────


def test_put_in_bowl_does_not_eat():
    c = Cat.new("測試", now=NOW)
    belly = c.belly
    msg = c.put_in_bowl(FOODS["dry"], 10)
    assert "10 克乾糧" in msg
    assert c.bowl_g == 10 and c.bowl_kcal == pytest.approx(38)
    assert c.belly == belly


def test_eat_from_bowl_bite_by_bite_matches_eating_at_once():
    a = Cat.new("一口一口", now=NOW)
    b = Cat.new("一次吃完", now=NOW)
    for c in (a, b):
        c.put_in_bowl(FOODS["wet"], 40)
    msgs = [a.eat_from_bowl(3, NOW) for _ in range(20)]
    b.finish_bowl(NOW)
    assert a.belly == pytest.approx(b.belly)
    assert a.w == pytest.approx(b.w)
    # 只有吃到碗空的那一口回傳訊息，整餐只記一筆
    done = [m for m in msgs if m]
    assert done == ["一口一口 吃了 40 克（40 kcal）"]
    assert a.log[-1][1] == "吃了 40 克（40 kcal）"
    assert a.meal_g == a.meal_kcal == a.meal_extra == 0


def test_eat_from_bowl_overfull_meal():
    c = Cat.new("測試", now=NOW)
    c.belly = need(c.w)  # 已經吃飽
    c.joy = 50
    c.put_in_bowl(FOODS["dry"], 20)
    msg = None
    while msg is None:
        msg = c.eat_from_bowl(5, NOW)
    assert "吃太撐" in msg
    assert c.joy == 45
    assert c.w == pytest.approx(4.0 + 76 / KCAL_PER_KG, rel=1e-3)


def test_eat_from_empty_bowl():
    c = Cat.new("測試", now=NOW)
    assert c.eat_from_bowl(5, NOW) is None


def test_advance():
    c = Cat.new("測試", now=NOW)
    born = c.born
    assert c.advance(2, NOW) == "快轉 2 小時。"
    assert c.born == born - 7200


def test_meal_survives_save_and_load():
    c = Cat.new("測試", now=NOW)
    c.put_in_bowl(FOODS["dry"], 10)
    c.eat_from_bowl(4, NOW)
    d = Cat.from_dict(c.to_dict(), NOW)
    assert d.meal_g == pytest.approx(4) and d.bowl_g == pytest.approx(6)


# ───────── core.run ─────────


def test_status_fields(store):
    r = core.run({"op": "status"}, store, now=NOW)
    assert r["ok"]
    cat = r["cat"]
    for k in ("name", "mood", "face", "bcs", "body", "belly_hours", "need_dry_g", "need_formula", "last_log", "bowl_g"):
        assert k in cat
    assert cat["body"] == "理想"
    assert cat["need_formula"] == "70 × 4.00^0.75 × 1.2 = 238 kcal"


def test_leftover_bowl_is_eaten_on_next_load(store):
    # 舊版存檔可能留著沒吃完的碗：下一次讀的時候牠已經吃完了
    d = Cat.new("測試", now=NOW).to_dict()
    d["bowl_g"], d["bowl_kcal"] = 10, 38
    store.path.write_text(json.dumps(d), encoding="utf-8")
    r = core.run({"op": "status"}, store, now=NOW + 60)
    assert r["cat"]["bowl_g"] == 0
    assert r["cat"]["last_log"].endswith("吃了 10 克（38 kcal）")


def test_errors(store):
    assert core.run({"op": "nope"}, store)["error"] == "不認得的操作：nope"
    assert "沒有這種食物" in core.run({"op": "feed", "food": "pizza"}, store)["error"]
    assert not core.run({"op": "feed", "grams": -3}, store)["ok"]
    assert not core.run({"op": "rename", "name": "  "}, store)["ok"]


def test_play_when_starving(store):
    with store.edit(NOW) as c:
        c.belly = 0
    r = core.run({"op": "play"}, store, now=NOW)
    assert not r["ok"] and "餓到不想玩" in r["error"]


def test_line(store):
    r = core.run({"op": "line"}, store, now=NOW)
    assert r["message"].endswith("4.00kg 肚子33%")


# ───────── 指令列 ─────────


def test_cli_parse():
    assert cli._parse([]) == {"op": "status"}
    assert cli._parse(["feed", "wet", "30"]) == {"op": "feed", "food": "wet", "grams": "30"}
    assert cli._parse(["rename", "小", "黑"]) == {"op": "rename", "name": "小 黑"}
    assert cli._parse(["adopt"]) == {"op": "adopt", "name": None}


def _neko_core(args, tmp_path, stdin=None):
    env = {"NEKO_STATE": str(tmp_path / "state.json"), "HOME": str(tmp_path), "PATH": ""}
    return subprocess.run(
        [sys.executable, "-m", "neko_api.cli", *args], input=stdin, capture_output=True, text=True, env=env, check=False
    )


def test_cli_one_shot(tmp_path):
    p = _neko_core(["feed", "wet"], tmp_path)
    assert p.returncode == 0
    assert json.loads(p.stdout)["message"].endswith("吃了 40 克（40 kcal）")
    p = _neko_core(["line"], tmp_path)
    assert p.returncode == 0 and "kg" in p.stdout and not p.stdout.startswith("{")
    p = _neko_core(["feed", "pizza"], tmp_path)
    assert p.returncode == 1 and not json.loads(p.stdout)["ok"]
