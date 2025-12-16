"""Tests for kitty_protocol module."""

import os
from unittest.mock import MagicMock, patch

import pytest

from meshtui.kitty_protocol import display_image, get_terminal_size, is_kitty_terminal


class TestIsKittyTerminal:
    """Tests for Kitty terminal detection."""

    def test_detects_kitty_from_term_env(self) -> None:
        """Test detection via TERM environment variable."""
        with patch.dict(os.environ, {"TERM": "xterm-kitty"}, clear=True):
            assert is_kitty_terminal() is True

    def test_detects_kitty_from_window_id(self) -> None:
        """Test detection via KITTY_WINDOW_ID."""
        with patch.dict(os.environ, {"KITTY_WINDOW_ID": "1"}, clear=True):
            assert is_kitty_terminal() is True

    def test_not_kitty_terminal(self) -> None:
        """Test detection fails for non-Kitty terminals."""
        with patch.dict(os.environ, {"TERM": "xterm-256color"}, clear=True):
            assert is_kitty_terminal() is False

    def test_no_env_variables(self) -> None:
        """Test detection fails when no relevant env vars are set."""
        with patch.dict(os.environ, {}, clear=True):
            assert is_kitty_terminal() is False


class TestGetTerminalSize:
    """Tests for terminal size detection."""

    @patch("meshtui.kitty_protocol.is_kitty_terminal")
    def test_raises_error_for_non_kitty_terminal(self, mock_is_kitty: MagicMock) -> None:
        """Test that non-Kitty terminals raise an error."""
        mock_is_kitty.return_value = False

        with pytest.raises(RuntimeError, match="Not running in a Kitty-compatible terminal"):
            get_terminal_size()


class TestDisplayImage:
    """Tests for image display functionality."""

    @patch("meshtui.kitty_protocol.is_kitty_terminal")
    @patch("meshtui.kitty_protocol.sys.stdout")
    def test_displays_small_image(self, mock_stdout: MagicMock, mock_is_kitty: MagicMock) -> None:
        """Test displaying a small image that fits in one chunk."""
        mock_is_kitty.return_value = True
        image_data = b"fake_png_data"

        display_image(image_data, 100, 100)

        # Should write the escape sequence
        assert mock_stdout.write.called
        assert mock_stdout.flush.called

        # Check that the control sequence was written
        calls = [call[0][0] for call in mock_stdout.write.call_args_list]
        assert any("\033_G" in call for call in calls)
        assert any("\033\\" in call for call in calls)

    @patch("meshtui.kitty_protocol.is_kitty_terminal")
    def test_raises_error_for_non_kitty_terminal(self, mock_is_kitty: MagicMock) -> None:
        """Test that non-Kitty terminals raise an error."""
        mock_is_kitty.return_value = False

        with pytest.raises(RuntimeError, match="Not running in a Kitty-compatible terminal"):
            display_image(b"data", 100, 100)

    @patch("meshtui.kitty_protocol.is_kitty_terminal")
    @patch("meshtui.kitty_protocol.sys.stdout")
    def test_handles_large_image_with_chunking(
        self, mock_stdout: MagicMock, mock_is_kitty: MagicMock
    ) -> None:
        """Test that large images are properly chunked."""
        mock_is_kitty.return_value = True
        # Create data that will require multiple chunks (4096 bytes per chunk after base64)
        large_data = b"x" * 5000

        display_image(large_data, 1000, 1000)

        # Should write multiple chunks
        calls = [call[0][0] for call in mock_stdout.write.call_args_list]
        # First chunk should have m=1 (more data coming)
        assert any("m=1" in call for call in calls)
        # Last chunk should have m=0 (no more data)
        assert any("m=0" in call for call in calls[-2:])
