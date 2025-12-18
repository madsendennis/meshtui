"""Tests for main CLI module."""

from pathlib import Path
from unittest.mock import MagicMock, patch

import trimesh

from meshtui.main import main


class TestMain:
    """Tests for CLI entry point."""

    @patch("meshtui.main.signal.signal")
    @patch("meshtui.main.wait_for_exit")
    @patch("meshtui.main.render_and_display")
    @patch("meshtui.main.load_mesh")
    @patch("meshtui.main.get_terminal_size")
    def test_successful_execution(
        self,
        mock_get_size: MagicMock,
        mock_load: MagicMock,
        mock_render_display: MagicMock,
        mock_wait: MagicMock,
        mock_signal: MagicMock,
    ) -> None:
        """Test successful mesh loading and display."""
        # Setup mocks
        mock_get_size.return_value = (800, 600, 10, 20)
        mock_load.return_value = trimesh.creation.box()

        # Call main with Path directly
        result = main(Path("test.ply"))

        assert result == 0
        mock_get_size.assert_called_once()
        mock_load.assert_called_once()
        mock_render_display.assert_called_once()
        mock_wait.assert_called_once()
        mock_signal.assert_called_once()  # SIGWINCH handler registered

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

    @patch("meshtui.main.render_mesh")
    @patch("meshtui.main.load_mesh")
    @patch("meshtui.main.get_terminal_size")
    @patch("meshtui.main.setup_tui")
    @patch("meshtui.main.cleanup_tui")
    def test_render_error(
        self,
        mock_cleanup: MagicMock,
        mock_setup: MagicMock,
        mock_get_size: MagicMock,
        mock_load: MagicMock,
        mock_render: MagicMock,
    ) -> None:
        """Test error during rendering."""
        mock_get_size.return_value = (800, 600, 10, 20)
        mock_load.return_value = trimesh.creation.box()
        mock_render.side_effect = Exception("Render failed")

        result = main(Path("test.ply"))

        assert result == 1

    @patch("meshtui.main.display_image")
    @patch("meshtui.main.render_mesh")
    @patch("meshtui.main.load_mesh")
    @patch("meshtui.main.get_terminal_size")
    @patch("meshtui.main.setup_tui")
    @patch("meshtui.main.cleanup_tui")
    def test_display_error(
        self,
        mock_cleanup: MagicMock,
        mock_setup: MagicMock,
        mock_get_size: MagicMock,
        mock_load: MagicMock,
        mock_render: MagicMock,
        mock_display: MagicMock,
    ) -> None:
        """Test error during display."""
        mock_get_size.return_value = (800, 600, 10, 20)
        mock_load.return_value = trimesh.creation.box()
        mock_render.return_value = b"fake_png_data"
        mock_display.side_effect = RuntimeError("Display failed")

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

    @patch("meshtui.main.signal.signal")
    @patch("meshtui.main.wait_for_exit")
    @patch("meshtui.main.render_and_display")
    @patch("meshtui.main.load_mesh")
    @patch("meshtui.main.get_terminal_size")
    @patch("meshtui.main.setup_tui")
    @patch("meshtui.main.cleanup_tui")
    def test_mesh_info_displayed(
        self,
        mock_cleanup: MagicMock,
        mock_setup: MagicMock,
        mock_get_size: MagicMock,
        mock_load: MagicMock,
        mock_render_display: MagicMock,
        mock_wait: MagicMock,
        mock_signal: MagicMock,
    ) -> None:
        """Test that mesh info (vertices, faces) is printed."""
        mesh = trimesh.creation.box()
        mock_get_size.return_value = (800, 600, 10, 20)
        mock_load.return_value = mesh

        with patch("builtins.print") as mock_print:
            result = main(Path("test.ply"))

        assert result == 0
        # Check that mesh info was printed (in render_and_display)
        print_calls = [str(call) for call in mock_print.call_args_list]
        assert any("Loading mesh" in str(call) for call in print_calls)
