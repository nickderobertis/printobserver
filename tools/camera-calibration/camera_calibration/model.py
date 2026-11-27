"""The camera model: OpenCV's fisheye (equidistant) projection with one term.

    Xc = R(rvec) @ Xw + t
    a, b = Xc.x / Xc.z, Xc.y / Xc.z;  r = hypot(a, b);  theta = atan(r)
    theta_d = theta * (1 + k1 theta^2)
    u = fx * a * theta_d / r + cx;  v = fy * b * theta_d / r + cy

The bed frame is the G-code's own: X right, Y back, Z up, in millimetres. With
the print at height `zc` the bed has dropped by `zc` (a bed-z printer), so a
point printed at height `h` sits at world `(x, y, h - zc)`. `cross_check` holds
this projection to `cv2.fisheye.projectPoints`.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np


def rodrigues(rvec: np.ndarray) -> np.ndarray:
    """The rotation matrix of a Rodrigues vector."""
    theta = float(np.linalg.norm(rvec))
    if theta < 1e-12:
        return np.eye(3)
    k = rvec / theta
    skew = np.array([[0, -k[2], k[1]], [k[2], 0, -k[0]], [-k[1], k[0], 0]])
    return np.eye(3) + np.sin(theta) * skew + (1 - np.cos(theta)) * skew @ skew


@dataclass(frozen=True)
class Camera:
    """Intrinsics with one distortion term, and the bed's pose."""

    fx: float
    fy: float
    cx: float
    cy: float
    k1: float
    rvec: np.ndarray
    tvec: np.ndarray

    def project(self, points: np.ndarray) -> np.ndarray:
        """World points (N, 3) to pixels (N, 2); a point behind the camera is NaN."""
        points = np.asarray(points, dtype=float).reshape(-1, 3)
        cam = points @ rodrigues(self.rvec).T + self.tvec
        z = cam[:, 2]
        with np.errstate(invalid="ignore", divide="ignore"):
            a, b = cam[:, 0] / z, cam[:, 1] / z
            r = np.hypot(a, b)
            theta = np.arctan(r)
            theta_d = theta * (1 + self.k1 * theta * theta)
            scale = np.where(r > 1e-12, theta_d / np.where(r > 1e-12, r, 1.0), 1.0)
            uv = np.stack([self.fx * a * scale + self.cx, self.fy * b * scale + self.cy], axis=1)
        uv[z <= 1e-6] = np.nan
        return uv

    def vector(self) -> np.ndarray:
        """The fitted parameters, in the order `of_vector` reads them."""
        return np.array([self.fx, self.fy, self.cx, self.cy, self.k1, *self.rvec, *self.tvec])

    @classmethod
    def of_vector(cls, x: np.ndarray) -> Camera:
        """A camera from its parameter vector."""
        return cls(x[0], x[1], x[2], x[3], x[4], np.asarray(x[5:8]), np.asarray(x[8:11]))


def cross_check(camera: Camera, points: np.ndarray) -> float:
    """The largest pixel difference between `Camera.project` and OpenCV's fisheye."""
    import cv2

    matrix = np.array([[camera.fx, 0, camera.cx], [0, camera.fy, camera.cy], [0, 0, 1]], float)
    ours = camera.project(points)
    seen = ~np.isnan(ours).any(axis=1)
    theirs, _ = cv2.fisheye.projectPoints(
        points[seen].reshape(-1, 1, 3).astype(np.float64),
        camera.rvec.reshape(3, 1).astype(np.float64),
        camera.tvec.reshape(3, 1).astype(np.float64),
        matrix,
        np.array([camera.k1, 0.0, 0.0, 0.0]).reshape(4, 1),
    )
    return float(np.abs(theirs.reshape(-1, 2) - ours[seen]).max())


def camera_position(camera: Camera) -> np.ndarray:
    """Where the camera is, in the bed frame."""
    return -rodrigues(camera.rvec).T @ camera.tvec
