"""测试 network.py 模块"""

import socket
import sys
import pytest
from unittest.mock import patch, MagicMock

from byksdk.network import (
    detect_iface_type,
    ensure_port_available,
    PortBusyError,
)


class TestDetectIfaceType:
    """测试 detect_iface_type 函数"""

    def test_wifi_wlan(self):
        t, virtual, prio = detect_iface_type("wlan0")
        assert t == "wifi"
        assert virtual is False
        assert prio == 10

    def test_wifi_wi_fi(self):
        t, virtual, prio = detect_iface_type("Wi-Fi")
        assert t == "wifi"
        assert virtual is False

    def test_wifi_chinese(self):
        t, virtual, prio = detect_iface_type("无线网络连接")
        assert t == "wifi"
        assert virtual is False

    def test_ethernet(self):
        t, virtual, prio = detect_iface_type("Ethernet0")
        assert t == "ethernet"
        assert virtual is False
        assert prio == 10

    def test_ethernet_chinese(self):
        t, virtual, prio = detect_iface_type("以太网")
        assert t == "ethernet"
        assert virtual is False

    def test_loopback(self):
        t, virtual, prio = detect_iface_type("Loopback Pseudo-Interface 1")
        assert t == "loopback"
        assert virtual is True
        assert prio == 100

    def test_vmware(self):
        t, virtual, prio = detect_iface_type("VMware Network Adapter VMnet8")
        assert t == "vmware"
        assert virtual is True
        assert prio == 30

    def test_virtualbox(self):
        t, virtual, prio = detect_iface_type("VirtualBox Host-Only Network")
        assert t == "virtualbox"
        assert virtual is True
        assert prio == 30

    def test_docker(self):
        t, virtual, prio = detect_iface_type("docker0")
        assert t == "container"
        assert virtual is True
        assert prio == 40

    def test_wsl(self):
        t, virtual, prio = detect_iface_type("vEthernet (WSL)")
        assert t == "container"
        assert virtual is True

    def test_bluetooth(self):
        t, virtual, prio = detect_iface_type("Bluetooth Network Connection")
        assert t == "bluetooth"
        assert virtual is True
        assert prio == 60

    def test_unknown(self):
        t, virtual, prio = detect_iface_type("tun0")
        assert t == "unknown"
        assert virtual is False
        assert prio == 50

    def test_case_insensitive(self):
        t, virtual, prio = detect_iface_type("WLAN")
        assert t == "wifi"


class TestGetPrivateNetworks:
    """测试 get_private_networks 函数"""

    @staticmethod
    def _run_with_mock(mock_addrs):
        """注入 mock psutil 并调用 get_private_networks"""
        mock_psutil = MagicMock()
        mock_psutil.__spec__ = MagicMock()
        mock_psutil.net_if_addrs.return_value = mock_addrs
        with patch.dict(sys.modules, {"psutil": mock_psutil}):
            from byksdk.network import get_private_networks
            return get_private_networks()

    @pytest.fixture
    def mock_psutil_addrs(self):
        """创建模拟的 psutil net_if_addrs 返回值"""

        def _make_addr(ip, family=socket.AF_INET):
            addr = MagicMock()
            addr.family = family
            addr.address = ip
            return addr

        return {
            "Ethernet0": [
                _make_addr("192.168.1.100"),
                _make_addr("fe80::1", family=socket.AF_INET6),
            ],
            "wlan0": [
                _make_addr("10.0.0.55"),
                _make_addr("169.254.1.1"),
            ],
            "lo": [
                _make_addr("127.0.0.1"),
            ],
            "docker0": [
                _make_addr("172.17.0.1"),
            ],
            "vmnet8": [
                _make_addr("172.30.0.1"),
            ],
        }

    def test_returns_private_ips(self, mock_psutil_addrs):
        """测试返回私有 IP 地址"""
        results = self._run_with_mock(mock_psutil_addrs)

        assert len(results) >= 2

        eth = [r for r in results if r["iface"] == "Ethernet0"]
        assert len(eth) == 1
        assert eth[0]["ips"] == ["192.168.1.100"]
        assert eth[0]["type"] == "ethernet"
        assert eth[0]["virtual"] is False

        wlan = [r for r in results if r["iface"] == "wlan0"]
        assert len(wlan) == 1
        assert wlan[0]["ips"] == ["10.0.0.55"]
        assert wlan[0]["type"] == "wifi"

        docker = [r for r in results if r["iface"] == "docker0"]
        assert len(docker) == 1
        assert docker[0]["ips"] == ["172.17.0.1"]
        assert docker[0]["type"] == "container"

    def test_filter_loopback(self, mock_psutil_addrs):
        """127.x.x.x 地址应被过滤"""
        results = self._run_with_mock(mock_psutil_addrs)

        lo = [r for r in results if r["iface"] == "lo"]
        assert len(lo) == 0

    def test_filter_link_local(self, mock_psutil_addrs):
        """169.254.x.x 链路本地地址应被过滤"""
        results = self._run_with_mock(mock_psutil_addrs)

        wlan = [r for r in results if r["iface"] == "wlan0"]
        assert len(wlan) == 1
        assert "169.254.1.1" not in wlan[0]["ips"]

    def test_sorted_by_priority(self, mock_psutil_addrs):
        """结果按优先级排序"""
        results = self._run_with_mock(mock_psutil_addrs)

        priorities = [r["priority"] for r in results]
        assert priorities == sorted(priorities)

    def test_fallback_to_localhost(self):
        """当没有局域网 IP 时返回 localhost"""
        results = self._run_with_mock({})

        assert len(results) == 1
        assert results[0]["iface"] == "localhost"
        assert results[0]["ips"] == ["127.0.0.1"]
        assert results[0]["type"] == "loopback"
        assert results[0]["virtual"] is True

    def test_172_16_to_31_range(self):
        """172.16.0.0 - 172.31.255.255 范围内的私有 IP"""

        def _make_addr(ip):
            addr = MagicMock()
            addr.family = socket.AF_INET
            addr.address = ip
            return addr

        addrs_in = {"eth0": [_make_addr("172.16.0.1")]}
        results = self._run_with_mock(addrs_in)
        assert len(results) == 1
        assert results[0]["ips"] == ["172.16.0.1"]

        addrs_out = {"eth0": [_make_addr("172.32.0.1")]}
        results = self._run_with_mock(addrs_out)
        assert len(results) == 1
        assert results[0]["iface"] == "localhost"


class TestEnsurePortAvailable:
    """测试 ensure_port_available 函数"""

    def test_port_available(self):
        """端口可用时不抛异常"""
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.bind(("127.0.0.1", 0))
            port = s.getsockname()[1]
        ensure_port_available(port, "127.0.0.1")

    def test_port_in_use(self):
        """端口被占用时抛出 PortBusyError"""
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            s.bind(("127.0.0.1", 0))
            port = s.getsockname()[1]

            with pytest.raises(PortBusyError) as exc_info:
                ensure_port_available(port, "127.0.0.1")
            assert "already in use" in str(exc_info.value)
            assert exc_info.value.port == port

    def test_port_busy_error_is_oserror(self):
        """PortBusyError 是 OSError 的子类"""
        assert issubclass(PortBusyError, OSError)

    def test_default_host(self):
        """默认 host 为 0.0.0.0"""
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.bind(("0.0.0.0", 0))
            port = s.getsockname()[1]
        ensure_port_available(port)