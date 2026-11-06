"""Small deterministic camera/clock fakes; no third-party module or hardware."""
import types


class Frame:
    def __init__(self, value=0, width=640, height=480, channels=3, dtype="uint8"):
        self.value = value
        self.shape = (height, width, channels) if channels != 1 else (height, width)
        self.dtype = types.SimpleNamespace(name=dtype)
        self.nbytes = width * height * channels

    def tobytes(self):
        return bytes([self.value]) * self.nbytes


class Clock:
    def __init__(self):
        self.now, self.sleeps = 0.0, []

    def monotonic(self):
        return self.now

    def sleep(self, seconds):
        self.sleeps.append(seconds)
        self.now += seconds


class Camera:
    def __init__(self, frames, opened=True):
        self.frames = iter(frames)
        self.opened = opened
        self.creations = self.closed = self.reads = 0
        self.calls, self.settings, self.threads = [], [], []
        self.on_open = self.on_read = None

    def factory(self, *args):
        self.creations += 1
        self.calls.append(args)
        if self.on_open:
            self.on_open()
        return self

    def isOpened(self):
        return self.opened

    def set(self, key, value):
        self.settings.append((key, value))
        return False  # The source must validate actual frames, not assume this worked.

    def read(self):
        self.reads += 1
        if self.on_read:
            self.on_read()
        value = next(self.frames)
        if isinstance(value, Exception):
            raise value
        if value is False:
            return False, None
        return True, value

    def release(self):
        self.closed += 1


def cv2(camera):
    def resize(frame, size, interpolation):
        if interpolation != 3:
            raise ValueError("Unexpected fixture interpolation")
        return Frame(frame.value, width=size[0], height=size[1])

    def gray(frame, code):
        if code != 6:
            raise ValueError("Unexpected fixture color conversion")
        return Frame(frame.value, width=frame.shape[1], height=frame.shape[0], channels=1)

    return types.SimpleNamespace(VideoCapture=camera.factory, resize=resize, cvtColor=gray,
        setNumThreads=camera.threads.append,
        INTER_AREA=3, COLOR_BGR2GRAY=6, CAP_FFMPEG=1900,
        CAP_PROP_FRAME_WIDTH=3, CAP_PROP_FRAME_HEIGHT=4,
        CAP_PROP_OPEN_TIMEOUT_MSEC=53, CAP_PROP_READ_TIMEOUT_MSEC=54, CAP_PROP_AUDIO_STREAM=58)
