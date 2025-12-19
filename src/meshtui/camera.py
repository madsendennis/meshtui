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

        # Target state for smoothing
        self.target_theta: float = self.theta
        self.target_phi: float = self.phi
        self.target_radius: float = self.radius

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

    @abstractmethod
    def reset_zoom(self) -> None:
        """Reset zoom level to default."""
        pass

    @abstractmethod
    def animate(self, smoothing_factor: float = 0.1) -> bool:
        """Animate camera towards target state. Returns True if updated."""
        pass

    def set_view_axis(self, axis: ViewAxis) -> None:
        """Set the camera to view from a specific axis."""
        axis = axis.lower()  # type: ignore
        if axis == "+z":
            self.target_theta = math.pi / 2.0
            self.target_phi = math.pi / 2.0
            self.up_vector = (0.0, 1.0, 0.0)
        elif axis == "-z":
            self.target_theta = -math.pi / 2.0
            self.target_phi = math.pi / 2.0
            self.up_vector = (0.0, 1.0, 0.0)
        elif axis == "+x":
            self.target_theta = 0.0
            self.target_phi = math.pi / 2.0
            self.up_vector = (0.0, 0.0, 1.0)
        elif axis == "-x":
            self.target_theta = math.pi
            self.target_phi = math.pi / 2.0
            self.up_vector = (0.0, 0.0, 1.0)
        elif axis == "+y":
            self.target_theta = math.pi / 2.0
            self.target_phi = math.pi / 2.0
            self.up_vector = (0.0, 0.0, 1.0)
        elif axis == "-y":
            self.target_theta = -math.pi / 2.0
            self.target_phi = math.pi / 2.0
            self.up_vector = (0.0, 0.0, 1.0)

        # Adjust target theta to be closest to current theta to avoid spinning
        diff = self.target_theta - self.theta
        diff = (diff + math.pi) % (2 * math.pi) - math.pi
        self.target_theta = self.theta + diff

    def orbit(self, delta_theta: float, delta_phi: float) -> None:
        """Orbit the camera around the target."""
        self.target_theta += delta_theta
        self.target_phi += delta_phi

        # Clamp phi to avoid gimbal lock
        self.target_phi = max(0.1, min(math.pi - 0.1, self.target_phi))

    def _animate_orbital(self, smoothing_factor: float) -> bool:
        updated = False
        epsilon = 0.001

        # Theta
        diff = self.target_theta - self.theta
        if abs(diff) > epsilon:
            self.theta += diff * smoothing_factor
            updated = True
        else:
            self.theta = self.target_theta

        # Phi
        diff = self.target_phi - self.phi
        if abs(diff) > epsilon:
            self.phi += diff * smoothing_factor
            updated = True
        else:
            self.phi = self.target_phi

        # Radius
        diff = self.target_radius - self.radius
        if abs(diff) > epsilon:
            self.radius += diff * smoothing_factor
            updated = True
        else:
            self.radius = self.target_radius

        if updated:
            self.update_position()

        return updated

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
        self.target_radius = radius
        self.update_position()


class PerspectiveCamera(Camera):
    def get_type(self) -> str:
        return "perspective"

    def zoom(self, factor: float) -> None:
        """Zoom by changing the orbital radius."""
        self.target_radius /= factor

    def reset_zoom(self) -> None:
        """Reset zoom level (handled by set_radius for perspective)."""
        pass

    def animate(self, smoothing_factor: float = 0.1) -> bool:
        return self._animate_orbital(smoothing_factor)


class OrthographicCamera(Camera):
    def __init__(self, target: tuple[float, float, float] = (0.0, 0.0, 0.0)):
        super().__init__(target)
        self.zoom_level = 1.0
        self.target_zoom_level = 1.0

    def get_type(self) -> str:
        return "orthographic"

    def zoom(self, factor: float) -> None:
        """Zoom by changing the orthographic scale."""
        # For ortho, "zoom in" means smaller view volume, so we multiply by factor
        # If factor < 1 (zoom in), zoom_level decreases.
        self.target_zoom_level *= factor

    def reset_zoom(self) -> None:
        """Reset zoom level to default."""
        self.zoom_level = 1.0
        self.target_zoom_level = 1.0

    def animate(self, smoothing_factor: float = 0.1) -> bool:
        updated = self._animate_orbital(smoothing_factor)

        epsilon = 0.001
        diff = self.target_zoom_level - self.zoom_level
        if abs(diff) > epsilon:
            self.zoom_level += diff * smoothing_factor
            updated = True
        else:
            self.zoom_level = self.target_zoom_level

        return updated
