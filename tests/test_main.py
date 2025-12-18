"""Tests for main CLI module."""

from pathlib import Path
from unittest.mock import MagicMock, patch

import trimesh

from meshtui.main import main


class TestMain:
    """Tests for CLI entry point."""

    @patch("meshtui.main.TUI")
    @patch("meshtui.main.load_mesh")
    @patch("meshtui.main.get_terminal_size")
    def test_successful_execution(
        self,
        mock_get_size: MagicMock,
        mock_load: MagicMock,
        mock_tui_cls: MagicMock,
    ) -> None:
        """Test successful mesh loading and display."""
        # Setup mocks
        mock_get_size.return_value = (800, 600, 10, 20)
        mock_load.return_value = trimesh.creation.box()

        mock_tui_instance = mock_tui_cls.return_value

        # Call main with Path directly
        result = main(Path("test.ply"))

        assert result == 0
        mock_get_size.assert_called_once()
        mock_load.assert_called_once()
        mock_tui_cls.assert_called_once_with(mock_load.return_value)
        mock_tui_instance.run.assert_called_once()

    @patch("meshtui.main.load_mesh")
    @patch("meshtui.main.get_terminal_size")
    def test_missing_file(self, mock_get_size: MagicMock, mock_load: MagicMock) -> None:
        """Test error when file doesn't exist."""
        mock_get_size.return_value = (800, 600, 10, 20)
        mock_load.side_effect = FileNotFoundError("File not found")

        result = main(Path("nonexistent.ply"))

        assert result == 1

    @patch("meshtui.main.get_terminal_size")
    def test_non_kitty_terminal(self, mock_get_size: MagicMock) -> None:
        """Test error when not in Kitty terminal."""
        mock_get_size.side_effect = RuntimeError(
            "Terminal does not support the Kitty graphics protocol"
        )

        result = main(Path("test.ply"))

        assert result == 1

    @patch("meshtui.main.load_mesh")
    @patch("meshtui.main.get_terminal_size")
    def test_unsupported_format(self, mock_get_size: MagicMock, mock_load: MagicMock) -> None:
        """Test error for unsupported file format."""
        mock_get_size.return_value = (800, 600, 10, 20)
        mock_load.side_effect = ValueError("Unsupported file format")

        result = main(Path("model.obj"))

        assert result == 1

    @patch("meshtui.main.TUI")
    @patch("meshtui.main.load_mesh")
    @patch("meshtui.main.get_terminal_size")
    def test_tui_error(
        self,
        mock_get_size: MagicMock,
        mock_load: MagicMock,
        mock_tui_cls: MagicMock,
    ) -> None:
        """Test error during TUI execution."""
        mock_get_size.return_value = (800, 600, 10, 20)
        mock_load.return_value = trimesh.creation.box()

        mock_tui_instance = mock_tui_cls.return_value
        mock_tui_instance.run.side_effect = Exception("TUI failed")

        result = main(Path("test.ply"))

        assert result == 1

    @patch("meshtui.main.load_mesh")
    @patch("meshtui.main.get_terminal_size")
    def test_keyboard_interrupt(self, mock_get_size: MagicMock, mock_load: MagicMock) -> None:
        """Test handling of keyboard interrupt."""
        mock_get_size.return_value = (800, 600, 10, 20)
        mock_load.side_effect = KeyboardInterrupt()

        result = main(Path("test.ply"))

        assert result == 0
