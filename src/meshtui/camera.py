import math
from abc import ABC, abstractmethod
from typing import Literal

from meshtui import config

ViewAxis = Literal["+x", "-x", "+y", "-y", "+z", "-z"]


class Camera(ABC):
    """Abstract base class for camera control."""

    def __init__(self, target: tuple[float, float, float] = (0.0, 0.0, 0.0)):
        self.target = target
        self.up_vector = (0.0, 1.0, 0.0)
        self.position = (0.0, 0.0, 1.0)

        # Orbital state
        self.theta: float = 0.0
        self.phi: float = math.pi / 2.0
        self.radius: float = 1.0

        # Configuration
        self.config = config.get_camera_config()
        self.orbital_config = config.get_orbital_camera_config()

    @abstractmethod
    def get_type(self) -> str:
        """Get the camera type string ('perspective' or 'orthographic')."""
        pass

    @abstractmethod
    def zoom(self, factor: float) -> None:
        """Zoom the camera by a factor."""
        pass

    def set_view_axis(self, axis: ViewAxis) -> None:
        """Set the camera to view from a specific axis."""
        axis = axis.lower()  # type: ignore
        if axis == "+z":
            self.theta = math.pi / 2.0
            self.phi = math.pi / 2.0
            self.up_vector = (0.0, 1.0, 0.0)
        elif axis == "-z":
            self.theta = -math.pi / 2.0
            self.phi = math.pi / 2.0
            self.up_vector = (0.0, 1.0, 0.0)
        elif axis == "+x":
            self.theta = 0.0
            self.phi = math.pi / 2.0
            self.up_vector = (0.0, 0.0, 1.0)
        elif axis == "-x":
            self.theta = math.pi
            self.phi = math.pi / 2.0
            self.up_vector = (0.0, 0.0, 1.0)
        elif axis == "+y":
            self.theta = math.pi / 2.0
            self.phi = math.pi / 2.0  # This is weird for +Y view.
            # Original code:
            # elif axis == "+y":
            #    # Camera at +Y looking toward -Y. Uses Z-up.
            #    _orbital_theta = math.pi / 2.0
            #    _orbital_phi = math.pi / 2.0
            self.up_vector = (0.0, 0.0, 1.0)
        elif axis == "-y":
            self.theta = -math.pi / 2.0
            self.phi = math.pi / 2.0
            self.up_vector = (0.0, 0.0, 1.0)

        # Re-calculate position based on new angles
        self.update_position()

    def orbit(self, delta_theta: float, delta_phi: float) -> None:
        """Orbit the camera around the target."""
        self.theta += delta_theta
        self.phi += delta_phi

        # Clamp phi to avoid gimbal lock
        self.phi = max(0.1, min(math.pi - 0.1, self.phi))

        self.update_position()

    def update_position(self) -> None:
        """Update Cartesian position from spherical coordinates."""
        sin_phi = math.sin(self.phi)
        cos_phi = math.cos(self.phi)
        cos_theta = math.cos(self.theta)
        sin_theta = math.sin(self.theta)

        tx, ty, tz = self.target
        r = self.radius

        # Handle different up vectors
        if self.up_vector == (0.0, 1.0, 0.0):  # Y-up
            x = tx + r * sin_phi * cos_theta
            y = ty + r * cos_phi
            z = tz + r * sin_phi * sin_theta
        elif self.up_vector == (0.0, 0.0, 1.0):  # Z-up
            x = tx + r * sin_phi * cos_theta
            y = ty + r * sin_phi * sin_theta
            z = tz + r * cos_phi
        else:
            # Default to Y-up
            x = tx + r * sin_phi * cos_theta
            y = ty + r * cos_phi
            z = tz + r * sin_phi * sin_theta

        self.position = (x, y, z)

    def set_target(self, target: tuple[float, float, float]) -> None:
        self.target = target
        self.update_position()

    def set_radius(self, radius: float) -> None:
        self.radius = radius
        self.update_position()


class PerspectiveCamera(Camera):
    def get_type(self) -> str:
        return "perspective"

    def zoom(self, factor: float) -> None:
        """Zoom by changing the orbital radius."""
        self.radius *= factor
        self.update_position()


class OrthographicCamera(Camera):
    def __init__(self, target: tuple[float, float, float] = (0.0, 0.0, 0.0)):
        super().__init__(target)
        self.zoom_level = 1.0

    def get_type(self) -> str:
        return "orthographic"

    def zoom(self, factor: float) -> None:
        """Zoom by changing the orthographic scale."""
        # For ortho, "zoom in" means smaller view volume, so we multiply by factor
        # If factor < 1 (zoom in), zoom_level decreases.
        self.zoom_level *= factor
