"""Drive the UI in a pseudo-terminal and print what a terminal would show (needs `pip install pyte`).
usage: drive.py <info.json>   (the fake daemon must already be running)"""
import os, pty, sys, time, select, struct, fcntl, termios, signal, pyte
signal.alarm(60)
info = sys.argv[1]
env = dict(os.environ, TERM='xterm-256color')
pid, fd = pty.fork()
if pid == 0:
    os.execvpe('node', ['node', '--import', 'tsx', 'src/main.tsx', '--info-file', info], env)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', 30, 100, 0, 0))
screen = pyte.Screen(100, 30); st = pyte.ByteStream(screen)
def pump(secs):
    end = time.time() + secs
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.03)
        if r:
            try: st.feed(os.read(fd, 65536))
            except OSError: return
def show(tag):
    print('---', tag)
    print('\n'.join(l.rstrip() for l in screen.display))
def send(b, wait=0.15):
    os.write(fd, b); pump(wait)
pump(2.0); show('start')
send(b'\x1b]11;rgb:1a1a/1f1f/2929\x07')
send(b'/obs'); show('slash menu')
send(b'\x1b'); send(b'ask the architect about hyperlinks\r', 0.3)   # text + Enter in ONE read
pump(5); show('after the reply streamed')
for _ in range(3): send(b'\x1b[<64;10;10M')
show('after the wheel')
send(b'\x1b[6~' * 3, 0.3)
send(b'line one'); send(b'\x1b\r'); send(b'line two'); show('multi-line prompt')
send(b'\x1b[200~pasted\r\ntext\x1b[201~'); show('after a paste')
send(b'\x1b'); send(b'\x1b[1;2C'); show('right deck'); send(b'\x1b[1;2D'); send(b'\x1b[1;2D'); show('left deck')
send(b'\x03', 0.5)
