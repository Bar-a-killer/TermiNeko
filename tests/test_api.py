import json

import pytest
from fastapi.testclient import TestClient

from neko_api.server import create_app
from neko_api.store import Store


@pytest.fixture
def client(tmp_path):
    return TestClient(create_app(Store(tmp_path / "state.json")))


def test_get_cat_creates_state(client, tmp_path):
    r = client.get("/cat")
    assert r.status_code == 200
    body = r.json()
    assert body["weight_kg"] == 4.0
    assert body["bcs"] == 5
    saved = json.loads((tmp_path / "state.json").read_text(encoding="utf-8"))
    assert saved["name"] == body["name"]


def test_reads_existing_rust_state(tmp_path):
    (tmp_path / "state.json").write_text(
        json.dumps({"name": "芝麻", "w": 4.2, "belly": 100, "joy": 50, "log": [["11-17 11:40", "領養了 芝麻"]]}),
        encoding="utf-8",
    )
    c = TestClient(create_app(Store(tmp_path / "state.json")))
    body = c.get("/cat").json()
    assert body["name"] == "芝麻"
    assert body["weight_kg"] == pytest.approx(4.2, abs=0.01)


def test_feed(client):
    r = client.post("/cat/feed", json={"food": "wet", "grams": 40})
    assert r.status_code == 200
    assert "40 克" in r.json()["message"]
    assert r.json()["cat"]["fed_today_kcal"] == pytest.approx(40, abs=0.1)


def test_feed_rejects_bad_food(client):
    assert client.post("/cat/feed", json={"food": "pizza"}).status_code == 422
    assert client.post("/cat/feed", json={"grams": -5}).status_code == 422


def test_pet_and_log(client):
    client.post("/cat/pet")
    client.post("/cat/play")
    log = client.get("/cat/log").json()
    assert log[-1]["message"] == "玩了球"


def test_play_when_starving(client):
    client.post("/cat/advance", json={"hours": 48})
    r = client.post("/cat/play")
    assert r.status_code == 409


def test_rename(client):
    r = client.post("/cat/rename", json={"name": "小黑"})
    assert r.json()["cat"]["name"] == "小黑"
    assert client.post("/cat/rename", json={"name": "   "}).status_code == 422


def test_adopt_needs_confirm(client):
    assert client.post("/cat/adopt", json={}).status_code == 400
    r = client.post("/cat/adopt", json={"name": "新貓", "confirm": True})
    assert r.json()["cat"]["name"] == "新貓"


def test_nutrition(client):
    body = client.get("/nutrition", params={"weight": 4}).json()
    assert body["need_kcal"] == pytest.approx(237.6, abs=0.1)
    assert body["wet_g"] == pytest.approx(237.6, abs=0.1)


def test_simulate(client):
    pts = client.post("/simulate", json={"daily_kcal": 300, "days": 30}).json()
    assert len(pts) == 31
    assert pts[-1]["w"] > pts[0]["w"]
