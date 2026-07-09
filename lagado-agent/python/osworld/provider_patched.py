import logging
import os
import platform
import time
import docker
import psutil
import requests
from filelock import FileLock
from pathlib import Path

from desktop_env.providers.base import Provider

logger = logging.getLogger("desktopenv.providers.docker.DockerProvider")
logger.setLevel(logging.INFO)

WAIT_TIME = 3
RETRY_INTERVAL = 1
LOCK_TIMEOUT = 10


class PortAllocationError(Exception):
    pass


class DockerProvider(Provider):
    def __init__(self, region: str):
        self.client = docker.from_env()
        self.server_port = None
        self.vnc_port = None
        self.chromium_port = None
        self.vlc_port = None
        self.container = None
        self.environment = {"DISK_SIZE": "32G", "RAM_SIZE": "3G", "CPU_CORES": "2"}  # trimmed for a 15Gi host under memory pressure (was 4G/4)

        temp_dir = Path(os.getenv('TEMP') if platform.system() == 'Windows' else '/tmp')
        self.lock_file = temp_dir / "docker_port_allocation.lck"
        self.lock_file.parent.mkdir(parents=True, exist_ok=True)

    def _get_used_ports(self):
        """Get all currently used ports (both system and Docker)."""
        # Get system ports
        system_ports = set(conn.laddr.port for conn in psutil.net_connections())
        
        # Get Docker container ports
        docker_ports = set()
        for container in self.client.containers.list():
            ports = container.attrs['NetworkSettings']['Ports']
            if ports:
                for port_mappings in ports.values():
                    if port_mappings:
                        docker_ports.update(int(p['HostPort']) for p in port_mappings)
        
        return system_ports | docker_ports

    def _get_available_port(self, start_port: int) -> int:
        """Find next available port starting from start_port."""
        used_ports = self._get_used_ports()
        port = start_port
        while port < 65354:
            if port not in used_ports:
                return port
            port += 1
        raise PortAllocationError(f"No available ports found starting from {start_port}")

    def _wait_for_vm_ready(self, timeout: int = 900):
        """Wait for VM to be ready by checking screenshot endpoint."""
        start_time = time.time()
        
        def check_screenshot():
            try:
                response = requests.get(
                    f"http://localhost:{self.server_port}/screenshot",
                    timeout=(10, 10)
                )
                return response.status_code == 200
            except Exception:
                return False

        while time.time() - start_time < timeout:
            if check_screenshot():
                return True
            logger.info("Checking if virtual machine is ready...")
            time.sleep(RETRY_INTERVAL)
        
        raise TimeoutError("VM failed to become ready within timeout period")

    def start_emulator(self, path_to_vm: str, headless: bool, os_type: str):
        # Use a single lock for all port allocation and container startup
        lock = FileLock(str(self.lock_file), timeout=LOCK_TIMEOUT)
        
        try:
            with lock:
                # Allocate all required ports
                self.vnc_port = self._get_available_port(8006)
                self.server_port = self._get_available_port(5000)
                self.chromium_port = self._get_available_port(9222)
                self.vlc_port = self._get_available_port(8080)
                # Lagado membrane: raw RFB (qemu-docker serves VNC on 5900 in-container).
                # 8006 is only the noVNC web wrapper; the pixel feed speaks raw RFB.
                self.rfb_port = self._get_available_port(5900)

                # Start container while still holding the lock
                # Check if KVM is available
                devices = []
                if os.path.exists("/dev/kvm"):
                    devices.append("/dev/kvm")
                    logger.info("KVM device found, using hardware acceleration")
                else:
                    self.environment["KVM"] = "N"
                    logger.warning("KVM device not found, running without hardware acceleration (will be slower)")
                # The nested QEMU VM needs a TUN device for networking + port-forwarding (host→guest :5000).
                # Without it qemu-docker falls back to usermode networking with NO port forwarding → the
                # controller can't reach the guest server. Pass the host's tun node (rootless podman).
                if os.path.exists("/dev/net/tun"):
                    devices.append("/dev/net/tun")

                self.container = self.client.containers.run(
                    "happysixd/osworld-docker",
                    environment=self.environment,
                    cap_add=["NET_ADMIN"],
                    devices=devices,
                    security_opt=["label=disable"],  # SELinux Enforcing blocks /dev/kvm + /dev/net/tun in rootless podman
                    sysctls={"net.ipv4.ip_forward": "1"},
                    privileged=True,  # qemu-docker NAT/MASQUERADE needs full privilege on this SELinux+nftables host  # qemu-docker needs IP forwarding for guest port-forward
                    volumes={
                        os.path.abspath(path_to_vm): {
                            "bind": "/System.qcow2",
                            "mode": "ro,z"  # SELinux relabel — required for rootless podman on Fedora (Enforcing)
                        }
                    },
                    ports={
                        8006: self.vnc_port,
                        5000: self.server_port,
                        9222: self.chromium_port,
                        8080: self.vlc_port,
                        5900: self.rfb_port,   # Lagado membrane: raw RFB for the pixel feed
                    },
                    detach=True
                )

                # Rootless podman (passt >= 2026-06 / rootlessport): published-port traffic is
                # delivered by rootlessport CONNECTING INSIDE the netns to the container's own
                # eth0 address — it never traverses PREROUTING, so qemu-docker's DNAT-to-guest
                # rules never match and host->guest :5000/:9222/:8080 reset (measured 2026-07-05:
                # nginx on :8006 answered, :5000 reset, PREROUTING counters zero; OUTPUT DNAT at
                # the container IP -> 200). Redirect those locally-destined ports to the guest in
                # the OUTPUT chain; MASQUERADE gives the guest a return path.
                # Detached + waits for the entrypoint's OWN nat rules first: qemu-docker's
                # network setup FLUSHES the nat table, so rules added immediately after run()
                # get wiped (measured: exec succeeded, rules gone 60s later). Its PREROUTING
                # DNAT (contains 20.20.20.21) appearing = setup done; append ours after.
                self.container.exec_run(["sh", "-c",
                    "for i in $(seq 90); do "
                    "iptables -t nat -L PREROUTING -n 2>/dev/null | grep -q 20.20.20.21 && break; "
                    "sleep 2; done; "
                    "set -- $(hostname -i); IP=$1; "
                    "for p in 5000 9222 8080; do "
                    "iptables -t nat -A OUTPUT -p tcp -d $IP --dport $p "
                    "-j DNAT --to-destination 20.20.20.21:$p; done; "
                    "iptables -t nat -A POSTROUTING -o dockerbridge -j MASQUERADE"],
                    detach=True)

            logger.info(f"Started container with ports - VNC: {self.vnc_port}, "
                       f"Server: {self.server_port}, Chrome: {self.chromium_port}, VLC: {self.vlc_port}")

            # Wait for VM to be ready
            self._wait_for_vm_ready()

        except Exception as e:
            # Clean up if anything goes wrong
            if self.container:
                try:
                    self.container.stop()
                    self.container.remove()
                except:
                    pass
            raise e

    def get_ip_address(self, path_to_vm: str) -> str:
        if not all([self.server_port, self.chromium_port, self.vnc_port, self.vlc_port]):
            raise RuntimeError("VM not started - ports not allocated")
        return f"localhost:{self.server_port}:{self.chromium_port}:{self.vnc_port}:{self.vlc_port}"

    def save_state(self, path_to_vm: str, snapshot_name: str):
        raise NotImplementedError("Snapshots not available for Docker provider")

    def revert_to_snapshot(self, path_to_vm: str, snapshot_name: str):
        self.stop_emulator(path_to_vm)

    def stop_emulator(self, path_to_vm: str, region=None, *args, **kwargs):
        # Note: region parameter is ignored for Docker provider
        # but kept for interface consistency with other providers
        if self.container:
            logger.info("Stopping VM...")
            try:
                self.container.stop()
                self.container.remove()
                time.sleep(WAIT_TIME)
            except Exception as e:
                logger.error(f"Error stopping container: {e}")
            finally:
                self.container = None
                self.server_port = None
                self.vnc_port = None
                self.chromium_port = None
                self.vlc_port = None
