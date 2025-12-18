import math

from meshtui.camera import OrthographicCamera, PerspectiveCamera


class TestCamera:
    def test_perspective_camera_init(self):
        cam = PerspectiveCamera()
        assert cam.get_type() == "perspective"
        assert cam.target == (0.0, 0.0, 0.0)
        assert cam.up_vector == (0.0, 1.0, 0.0)

    def test_orthographic_camera_init(self):
        cam = OrthographicCamera()
        assert cam.get_type() == "orthographic"
        assert cam.zoom_level == 1.0

    def test_set_view_axis(self):
        cam = PerspectiveCamera()

        cam.set_view_axis("+z")
        assert cam.theta == math.pi / 2.0
        assert cam.phi == math.pi / 2.0

        cam.set_view_axis("-z")
        assert cam.theta == -math.pi / 2.0
        assert cam.phi == math.pi / 2.0

        cam.set_view_axis("+y")
        assert cam.up_vector == (0.0, 0.0, 1.0)

    def test_orbit(self):
        cam = PerspectiveCamera()
        initial_theta = cam.theta
        initial_phi = cam.phi

        cam.orbit(0.1, 0.1)
        assert cam.theta == initial_theta + 0.1
        assert cam.phi == initial_phi + 0.1

    def test_zoom_perspective(self):
        cam = PerspectiveCamera()
        initial_radius = cam.radius
        cam.zoom(0.5)
        assert cam.radius == initial_radius * 0.5

    def test_zoom_orthographic(self):
        cam = OrthographicCamera()
        initial_zoom = cam.zoom_level
        cam.zoom(2.0)
        assert cam.zoom_level == initial_zoom * 2.0
