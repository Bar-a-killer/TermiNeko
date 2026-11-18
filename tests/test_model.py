import pytest

from neko_api.model import FLOOR, FOODS, IDEAL, KCAL_PER_KG, Cat, Mood, need, simulate

NOW = 1_800_000_000.0


def cat(**kw) -> Cat:
    c = Cat.new("測試", now=NOW)
    for k, v in kw.items():
        setattr(c, k, v)
    return c


def test_need_formula():
    assert need(4.0) == pytest.approx(70 * 4**0.75 * 1.2)
    assert need(4.0) == pytest.approx(237.6, abs=0.1)


def test_new_cat_defaults():
    c = cat()
    assert c.w == IDEAL
    assert c.belly == pytest.approx(need(IDEAL) / 3)
    assert c.log[-1][1] == "領養了 測試"


def test_one_day_without_food_loses_weight():
    c = cat(belly=0.0)
    c.pass_time(86400, NOW)
    assert c.w == pytest.approx(IDEAL - need(IDEAL) / KCAL_PER_KG)


def test_weight_has_floor():
    c = cat(belly=0.0)
    c.pass_time(86400 * 365, NOW)
    assert c.w == FLOOR
    assert "鄰居" in c.log[-1][1]


def test_overeating_becomes_weight():
    c = cat(belly=0.0)
    extra = c.eat(need(IDEAL) + 700)
    assert extra == pytest.approx(700)
    assert c.w == pytest.approx(IDEAL + 0.1)


def test_feed_and_message():
    c = cat(belly=0.0)
    msg = c.feed(FOODS["wet"], 40, NOW)
    assert "40 克（40 kcal）" in msg
    assert c.bowl_g == 0 and c.belly == pytest.approx(40)


def test_treat_adds_joy():
    c = cat(joy=50.0, belly=0.0)
    c.feed(FOODS["treat"], 4, NOW)
    assert c.joy == pytest.approx(50 + 10 + 6)


def test_play_refuses_when_starving():
    with pytest.raises(ValueError):
        cat(belly=0.0).play()


@pytest.mark.parametrize("w,bcs", [(4.0, 5), (4.4, 6), (3.6, 4), (10.0, 9), (1.0, 1)])
def test_bcs(w, bcs):
    assert cat(w=w).bcs() == bcs


def test_mood():
    n = need(IDEAL)
    assert cat(belly=0.0).mood() is Mood.STARVING
    assert cat(belly=n * 0.05).mood() is Mood.HUNGRY
    assert cat(belly=n * 0.95).mood() is Mood.FULL
    assert cat(belly=n * 0.5, joy=10).mood() is Mood.BORED
    assert cat(belly=n * 0.5, joy=90).mood() is Mood.HAPPY
    assert cat(belly=n * 0.5, joy=50).mood() is Mood.OK
    assert not hasattr(Mood, "SLEEP")  # 不會因為時間自己睡


def test_roundtrip_dict():
    c = cat()
    d = c.to_dict()
    assert Cat.from_dict(d, NOW) == c


def test_broken_state_readopts():
    assert Cat.from_dict({"w": -1}, NOW).w == IDEAL
    assert Cat.from_dict({}, NOW).w == IDEAL


def test_simulate_maintenance_is_stable():
    pts = simulate(4.0, need(4.0), 60)
    assert pts[-1]["w"] == pytest.approx(4.0, abs=0.01)


def test_simulate_overfeeding_gains():
    pts = simulate(4.0, need(4.0) * 1.2, 90)
    assert pts[-1]["w"] > 4.3
    assert len(pts) == 91
