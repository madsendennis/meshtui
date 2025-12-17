"""Tests for kitty_protocol module."""

from unittest.mock import MagicMock, patch

import pytest

from meshtui.kitty_protocol import display_image, get_terminal_size, is_kitty_terminal


class TestIsKittyTerminal:
    """Tests for Kitty graphics protocol detection."""

    @patch("meshtui.kitty_protocol.sys.stdout.isatty")
    @patch("meshtui.kitty_protocol.select.select")
    @patch("meshtui.kitty_protocol.os.read")
    @patch("meshtui.kitty_protocol.fcntl.fcntl")
    @patch("meshtui.kitty_protocol.sys.stdout.flush")
    @patch("meshtui.kitty_protocol.sys.stdout.write")
    @patch("meshtui.kitty_protocol.sys.stdin.fileno")
    @patch("meshtui.kitty_protocol.termios.tcgetattr")
    @patch("meshtui.kitty_protocol.termios.tcsetattr")
    @patch("meshtui.kitty_protocol.tty.setraw")
    def test_detects_kitty_protocol_support(
        self,
        mock_setraw: MagicMock,
        mock_tcsetattr: MagicMock,
        mock_tcgetattr: MagicMock,
        mock_fileno: MagicMock,
        mock_write: MagicMock,
        mock_flush: MagicMock,
        mock_fcntl: MagicMock,
        mock_os_read: MagicMock,
        mock_select: MagicMock,
        mock_isatty: MagicMock,
    ) -> None:
        """Test detection of Kitty graphics protocol support."""
        mock_isatty.return_value = True
        mock_fileno.return_value = 0
        mock_tcgetattr.return_value = "fake_settings"
        mock_fcntl.return_value = 0
        mock_select.return_value = ([True], [], [])
        mock_os_read.return_value = b"\033_Gi=1,OK\033\\"

        # Reset the cache before test
        import meshtui.kitty_protocol

        meshtui.kitty_protocol._kitty_protocol_supported = None

        assert is_kitty_terminal() is True

    @patch("meshtui.kitty_protocol.sys.stdout.isatty")
    def test_not_a_tty(self, mock_isatty: MagicMock) -> None:
        """Test detection fails when not a TTY."""
        # Reset the cache before test
        import meshtui.kitty_protocol

        meshtui.kitty_protocol._kitty_protocol_supported = None

        mock_isatty.return_value = False
        assert is_kitty_terminal() is False

    @patch("meshtui.kitty_protocol.sys.stdout.isatty")
    @patch("meshtui.kitty_protocol.select.select")
    @patch("meshtui.kitty_protocol.fcntl.fcntl")
    @patch("meshtui.kitty_protocol.sys.stdout.flush")
    @patch("meshtui.kitty_protocol.sys.stdout.write")
    @patch("meshtui.kitty_protocol.sys.stdin.fileno")
    @patch("meshtui.kitty_protocol.termios.tcgetattr")
    @patch("meshtui.kitty_protocol.termios.tcsetattr")
    @patch("meshtui.kitty_protocol.tty.setraw")
    def test_no_protocol_response(
        self,
        mock_setraw: MagicMock,
        mock_tcsetattr: MagicMock,
        mock_tcgetattr: MagicMock,
        mock_fileno: MagicMock,
        mock_write: MagicMock,
        mock_flush: MagicMock,
        mock_fcntl: MagicMock,
        mock_select: MagicMock,
        mock_isatty: MagicMock,
    ) -> None:
        """Test detection fails when terminal doesn't respond."""
        # Reset the cache before test
        import meshtui.kitty_protocol

        meshtui.kitty_protocol._kitty_protocol_supported = None

        mock_isatty.return_value = True
        mock_fileno.return_value = 0
        mock_tcgetattr.return_value = "fake_settings"
        mock_fcntl.return_value = 0
        mock_select.return_value = ([], [], [])  # No response (timeout)

        assert is_kitty_terminal() is False

    @patch("meshtui.kitty_protocol.sys.stdout.isatty")
    @patch("meshtui.kitty_protocol.select.select")
    @patch("meshtui.kitty_protocol.os.read")
    @patch("meshtui.kitty_protocol.fcntl.fcntl")
    @patch("meshtui.kitty_protocol.sys.stdout.flush")
    @patch("meshtui.kitty_protocol.sys.stdout.write")
    @patch("meshtui.kitty_protocol.sys.stdin.fileno")
    @patch("meshtui.kitty_protocol.termios.tcgetattr")
    @patch("meshtui.kitty_protocol.termios.tcsetattr")
    @patch("meshtui.kitty_protocol.tty.setraw")
    def test_invalid_protocol_response(
        self,
        mock_setraw: MagicMock,
        mock_tcsetattr: MagicMock,
        mock_tcgetattr: MagicMock,
        mock_fileno: MagicMock,
        mock_write: MagicMock,
        mock_flush: MagicMock,
        mock_fcntl: MagicMock,
        mock_os_read: MagicMock,
        mock_select: MagicMock,
        mock_isatty: MagicMock,
    ) -> None:
        """Test detection fails when response is not Kitty protocol."""
        # Reset the cache before test
        import meshtui.kitty_protocol

        meshtui.kitty_protocol._kitty_protocol_supported = None

        mock_isatty.return_value = True
        mock_fileno.return_value = 0
        mock_tcgetattr.return_value = "fake_settings"
        mock_fcntl.return_value = 0
        mock_select.return_value = ([True], [], [])
        mock_os_read.return_value = b"some random response"

        assert is_kitty_terminal() is False


class TestGetTerminalSize:
    """Tests for terminal size detection."""

    @patch("meshtui.kitty_protocol.is_kitty_terminal")
    def test_raises_error_for_non_kitty_terminal(self, mock_is_kitty: MagicMock) -> None:
        """Test that non-Kitty terminals raise an error."""
        mock_is_kitty.return_value = False

        with pytest.raises(
            RuntimeError, match="Terminal does not support the Kitty graphics protocol"
        ):
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

        with pytest.raises(
            RuntimeError, match="Terminal does not support the Kitty graphics protocol"
        ):
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
