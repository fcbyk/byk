"""网络工具函数"""

import socket
from typing import List, Dict, Any

from ._internal import requires


# 网络接口分类规则：(关键词列表，类型名称，是否虚拟接口，优先级)
IFACE_RULES = [
    (['vmware', 'vmnet'],         'vmware',     True,  30),
    (['vbox', 'virtualbox'],      'virtualbox', True,  30),
    (['docker', 'wsl'],           'container',  True,  40),
    (['bluetooth'],               'bluetooth',  True,  60),
    (['ethernet', '以太网'],       'ethernet',   False, 10),
    (['wlan', 'wi-fi', '无线'],   'wifi',       False, 10),
    (['loopback'],                'loopback',   True,  100),
]


def detect_iface_type(iface: str) -> tuple[str, bool, int]:
    """检测网络接口类型"""
    lname = iface.lower()
    for keywords, t, virtual, prio in IFACE_RULES:
        if any(k in lname for k in keywords):
            return t, virtual, prio
    return 'unknown', False, 50


@requires("psutil", version="5.9.0")
def get_private_networks() -> List[Dict[str, Any]]:
    """获取所有局域网 IP 地址信息（需 psutil>=5.9.0）"""
    import psutil

    results = []
    interfaces = psutil.net_if_addrs()

    for iface, addrs in interfaces.items():
        ips = []
        iface_type, is_virtual, priority = detect_iface_type(iface)

        for addr in addrs:
            if addr.family != socket.AF_INET:
                continue

            ip = addr.address

            # 跳过回环地址和链路本地地址
            if ip.startswith('127.'):
                continue
            if ip.startswith('169.254.'):
                continue
            # 只保留私有 IP 地址 (RFC 1918)
            if ip.startswith('10.') or ip.startswith('192.168.'):
                ips.append(ip)
            elif ip.startswith('172.'):
                second_octet = int(ip.split('.')[1])
                if 16 <= second_octet <= 31:
                    ips.append(ip)

        if ips:
            results.append({
                "iface": iface,
                "ips": ips,
                "type": iface_type,
                "virtual": is_virtual,
                "priority": priority
            })

    # 按优先级排序（数字越小优先级越高）
    results.sort(key=lambda x: x['priority'])
    
    # 如果没有找到任何局域网 IP，返回 localhost
    if not results:
        results.append({
            "iface": "localhost",
            "ips": ["127.0.0.1"],
            "type": "loopback",
            "virtual": True,
            "priority": 100
        })
    
    return results


def ensure_port_available(port: int, host: str = "0.0.0.0") -> None:
    """检查端口是否可用，不可用时抛出 PortBusyError

    Raises:
        PortBusyError: 端口已被占用
    """
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            s.bind((host, int(port)))
    except OSError as e:
        raise PortBusyError(port, host) from e


class PortBusyError(OSError):
    """端口被占用异常"""

    def __init__(self, port: int, host: str = "0.0.0.0") -> None:
        self.port = port
        self.host = host
        super().__init__(f"Port {port} on {host} is already in use")