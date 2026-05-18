"""Tests for pokecon.events — EventBus Python wrapper."""

from __future__ import annotations

from typing import Any

import pytest
from pokecon.events import (
    autocmd_clear,
    autocmd_group,
    define_event,
    emit,
    get_event_schema,
    get_group_names,
    get_handler_count,
    get_handler_ids,
    has_handlers,
    num_event_types,
    off,
    off_all,
    on,
    once,
)


@pytest.fixture(autouse=True)
def _reset_event_state() -> None:
    """Reset all event handler state before each test.

    Module-level state is shared across tests, so we clear it to
    ensure test isolation.
    """
    off_all()
    # Also clear event schemas
    import pokecon.events as _evt_mod

    _evt_mod._event_schemas.clear()  # type: ignore[attr-defined]


class TestOnOff:
    """Test basic subscription and unsubscription."""

    def test_on_returns_handler_id(self) -> None:
        def handler(evt: str, data: str) -> None:
            pass

        hid = on("test_event", handler)
        assert isinstance(hid, str)
        assert len(hid) > 0

    def test_on_callback_invoked_on_emit(self) -> None:
        calls: list[tuple[str, str]] = []

        def handler(evt: str, data: str) -> None:
            calls.append((evt, data))

        on("test_event", handler)
        emit("test_event", "hello")

        assert len(calls) == 1
        assert calls[0] == ("test_event", "hello")

    def test_on_callback_not_invoked_for_different_event(self) -> None:
        calls: list[str] = []

        def handler(evt: str, data: str) -> None:
            calls.append(evt)

        on("event_a", handler)
        emit("event_b", "data")

        assert len(calls) == 0

    def test_off_removes_handler(self) -> None:
        calls: list[str] = []

        def handler(evt: str, data: str) -> None:
            calls.append(evt)

        hid = on("test_event", handler)
        off(hid)
        emit("test_event", "data")

        assert len(calls) == 0

    def test_multiple_handlers_same_event(self) -> None:
        results: list[int] = []

        def make_handler(idx: int):
            def handler(evt: str, data: str) -> None:
                results.append(idx)

            return handler

        on("test_event", make_handler(1))
        on("test_event", make_handler(2))
        emit("test_event", "data")

        assert sorted(results) == [1, 2]

    def test_multiple_handlers_different_events(self) -> None:
        results: list[str] = []

        def handler_a(evt: str, data: str) -> None:
            results.append("a")

        def handler_b(evt: str, data: str) -> None:
            results.append("b")

        on("event_a", handler_a)
        on("event_b", handler_b)
        emit("event_a", "data")

        assert results == ["a"]

    def test_off_all_removes_everything(self) -> None:
        calls: list[int] = []

        def handler1(evt: str, data: str) -> None:
            calls.append(1)

        def handler2(evt: str, data: str) -> None:
            calls.append(2)

        on("event_1", handler1)
        on("event_2", handler2)
        off_all()
        emit("event_1", "data")
        emit("event_2", "data")

        assert len(calls) == 0
        assert get_handler_count() == 0

    def test_data_passthrough(self) -> None:
        captured: list[str] = []

        def handler(evt: str, data: str) -> None:
            captured.append(data)

        on("test_event", handler)
        emit("test_event", "payload_value")

        assert captured == ["payload_value"]

    def test_none_data(self) -> None:
        captured: list[Any] = []

        def handler(evt: str, data: str) -> None:
            captured.append(data)

        on("test_event", handler)
        emit("test_event")  # no data argument

        # None becomes "null" after JSON serialization or str -> "None"
        # depending on how _serialize_data handles None
        assert len(captured) == 1


class TestOnce:
    """Test one-time handlers."""

    def test_once_handler_fires_once(self) -> None:
        call_count: int = 0

        def handler(evt: str, data: str) -> None:
            nonlocal call_count
            call_count += 1

        once("test_event", handler)
        emit("test_event", "first")
        emit("test_event", "second")

        assert call_count == 1

    def test_once_handler_id_can_be_used_with_off(self) -> None:
        call_count: int = 0

        def handler(evt: str, data: str) -> None:
            nonlocal call_count
            call_count += 1

        hid = once("test_event", handler)
        off(hid)  # cancel before event fires
        emit("test_event", "data")

        assert call_count == 0


class TestPatternFiltering:
    """Test fnmatch-style pattern filtering."""

    def test_pattern_wildcard_matches(self) -> None:
        matched: list[str] = []

        def handler(evt: str, data: str) -> None:
            matched.append(evt)

        on("battle_*", handler, pattern="battle_*")
        emit("battle_start", "data")
        emit("battle_end", "data")
        emit("menu_open", "data")  # should NOT match

        assert matched == ["battle_start", "battle_end"]

    def test_pattern_question_mark(self) -> None:
        matched: list[str] = []

        def handler(evt: str, data: str) -> None:
            matched.append(evt)

        on("evt?", handler, pattern="evt?")
        emit("evt1", "data")
        emit("evt2", "data")
        emit("evt12", "data")  # should NOT match (too long)

        assert matched == ["evt1", "evt2"]

    def test_pattern_no_match(self) -> None:
        matched: list[str] = []

        def handler(evt: str, data: str) -> None:
            matched.append(evt)

        on("nonexistent_*", handler, pattern="nonexistent_*")
        emit("actual_event", "data")

        assert len(matched) == 0

    def test_pattern_with_exact_name_fallback(self) -> None:
        """When a pattern is set, event_name is a label; pattern controls matching."""
        matched: list[str] = []

        def handler(evt: str, data: str) -> None:
            matched.append(evt)

        # event_name = wildcard_label, pattern = actual filter
        on("wildcard", handler, pattern="test_*")
        emit("test_hello", "data")
        emit("other", "data")

        assert matched == ["test_hello"]


class TestPhaseFilter:
    """Test phase metadata passthrough."""

    def test_phase_passed_to_callback(self) -> None:
        captured: list[tuple[str, str, str]] = []

        def handler(evt: str, data: str, phase: str) -> None:
            captured.append((evt, data, phase))

        on("test_event", handler, phase="capture")
        emit("test_event", "payload")

        assert len(captured) == 1
        assert captured[0] == ("test_event", "payload", "capture")


class TestAutocmdGroup:
    """Test group-based handler management."""

    def test_autocmd_clear_removes_group_handlers(self) -> None:
        calls: list[str] = []

        def handler1(evt: str, data: str) -> None:
            calls.append("h1")

        def handler2(evt: str, data: str) -> None:
            calls.append("h2")

        on("evt1", handler1, group="test_group")
        on("evt2", handler2, group="test_group")
        autocmd_clear("test_group")

        emit("evt1", "data")
        emit("evt2", "data")

        assert len(calls) == 0

    def test_autocmd_clear_only_removes_specified_group(self) -> None:
        calls: list[str] = []

        def handler1(evt: str, data: str) -> None:
            calls.append("h1")

        def handler2(evt: str, data: str) -> None:
            calls.append("h2")

        on("evt1", handler1, group="group_a")
        on("evt2", handler2, group="group_b")
        autocmd_clear("group_a")

        emit("evt1", "data")
        emit("evt2", "data")

        assert calls == ["h2"]

    def test_autocmd_group_context_manager(self) -> None:
        calls: list[str] = []

        def handler1(evt: str, data: str) -> None:
            calls.append("h1")

        def handler2(evt: str, data: str) -> None:
            calls.append("h2")

        with autocmd_group("ctx_group"):
            on("evt1", handler1)
            on("evt2", handler2)

        autocmd_clear("ctx_group")

        emit("evt1", "data")
        emit("evt2", "data")

        assert len(calls) == 0

    def test_autocmd_group_no_interference_outside_context(self) -> None:
        calls: list[str] = []

        def handler(evt: str, data: str) -> None:
            calls.append("h1")

        # Outside context — no group assigned
        on("evt_outside", handler)

        with autocmd_group("ctx"):
            on("evt_inside", handler)

        autocmd_clear("ctx")

        emit("evt_outside", "data")
        emit("evt_inside", "data")

        # The outside handler should still fire
        assert calls == ["h1"]

    def test_get_group_names(self) -> None:
        def handler(evt: str, data: str) -> None:
            pass

        on("evt1", handler, group="g1")
        on("evt2", handler, group="g2")

        names = get_group_names()
        assert "g1" in names
        assert "g2" in names

    def test_autocmd_clear_nonexistent_group(self) -> None:
        # Should not raise
        autocmd_clear("nonexistent_group")


class TestDefineEvent:
    """Test event schema definition."""

    def test_define_and_get_schema(self) -> None:
        schema = {"type": "object", "properties": {"name": {"type": "string"}}}
        define_event("my_event", schema)
        result = get_event_schema("my_event")
        assert result == schema

    def test_get_schema_nonexistent(self) -> None:
        result = get_event_schema("nonexistent")
        assert result is None

    def test_define_overwrites_schema(self) -> None:
        define_event("event", {"old": True})
        define_event("event", {"new": True})
        assert get_event_schema("event") == {"new": True}


class TestQueryFunctions:
    """Test has_handlers, num_event_types, get_handler_count, get_handler_ids."""

    def test_has_handlers_true(self) -> None:
        def handler(evt: str, data: str) -> None:
            pass

        on("test_event", handler)
        assert has_handlers("test_event") is True

    def test_has_handlers_false(self) -> None:
        assert has_handlers("nonexistent") is False

    def test_has_handlers_with_pattern(self) -> None:
        def handler(evt: str, data: str) -> None:
            pass

        on("test_*", handler, pattern="test_*")
        assert has_handlers("test_hello") is True
        assert has_handlers("other") is False

    def test_num_event_types(self) -> None:
        def handler(evt: str, data: str) -> None:
            pass

        initial = num_event_types()
        on("evt1", handler)
        on("evt2", handler)
        assert num_event_types() >= initial + 2

    def test_get_handler_count(self) -> None:
        def handler(evt: str, data: str) -> None:
            pass

        initial = get_handler_count()
        h1 = on("evt1", handler)
        h2 = on("evt2", handler)
        assert get_handler_count() == initial + 2

        off(h1)
        assert get_handler_count() == initial + 1

        off(h2)
        assert get_handler_count() == initial

    def test_get_handler_ids(self) -> None:
        def handler(evt: str, data: str) -> None:
            pass

        h1 = on("evt1", handler)
        h2 = on("evt2", handler)
        ids = get_handler_ids()

        assert h1 in ids
        assert h2 in ids


class TestErrorHandling:
    """Test resilience against edge cases."""

    def test_handler_that_raises_does_not_break_other_handlers(self) -> None:
        results: list[int] = []

        def failing_handler(evt: str, data: str) -> None:
            msg = "intentional failure"
            raise RuntimeError(msg)

        def good_handler(evt: str, data: str) -> None:
            results.append(42)

        on("test_event", failing_handler)
        on("test_event", good_handler)
        emit("test_event", "data")

        # The good handler should still be called
        assert results == [42]

    def test_emit_with_none_data(self) -> None:
        captured: list[Any] = []

        def handler(evt: str, data: str) -> None:
            captured.append(data)

        on("test_event", handler)
        emit("test_event", None)

        assert len(captured) == 1

    def test_off_nonexistent_handler(self) -> None:
        # Should not raise
        off("nonexistent_id")

    def test_emit_no_handlers(self) -> None:
        # Should not raise
        emit("nonexistent_event", "data")

    def test_double_off_idempotent(self) -> None:
        calls: list[str] = []

        def handler(evt: str, data: str) -> None:
            calls.append("called")

        hid = on("test_event", handler)
        off(hid)
        off(hid)  # second off should be a no-op
        emit("test_event", "data")

        assert len(calls) == 0


class TestIntegration:
    """Integration-level tests combining multiple features."""

    def test_on_once_and_pattern_together(self) -> None:
        """Mixed exact, once, and pattern handlers work together."""
        results: dict[str, int] = {}

        def exact_handler(evt: str, data: str) -> None:
            results["exact"] = results.get("exact", 0) + 1

        def once_handler(evt: str, data: str) -> None:
            results["once"] = results.get("once", 0) + 1

        def pattern_handler(evt: str, data: str) -> None:
            results["pattern"] = results.get("pattern", 0) + 1

        on("shared_event", exact_handler)
        once("shared_event", once_handler)
        on("sha*", pattern_handler, pattern="sha*")

        emit("shared_event", "first")
        emit("shared_event", "second")

        assert results.get("exact") == 2
        assert results.get("once") == 1  # only fires once
        assert results.get("pattern") == 2  # pattern matches both times

    def test_off_all_resets_state_completely(self) -> None:
        def handler(evt: str, data: str) -> None:
            pass

        on("evt1", handler, group="g1")
        on("evt2", handler, group="g2")
        define_event("evt1", {"type": "test"})

        off_all()

        assert get_handler_count() == 0
        assert get_group_names() == []

    def test_large_number_of_handlers(self) -> None:
        """Register and fire many handlers."""
        count: int = 0

        def handler(evt: str, data: str) -> None:
            nonlocal count
            count += 1

        handlers = []
        for i in range(100):
            hid = on(f"event_{i}", handler)
            handlers.append(hid)

        for i in range(100):
            emit(f"event_{i}", "data")

        assert count == 100

        # Clean up
        for hid in handlers:
            off(hid)

        assert get_handler_count() == 0
