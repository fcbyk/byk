"""pytest 公共 fixtures"""

import pytest
import sys
from unittest.mock import patch, MagicMock


@pytest.fixture
def mock_psutil():
    """模拟 psutil 库"""
    with patch.dict(sys.modules, {'psutil': MagicMock()}):
        import psutil
        yield psutil


@pytest.fixture
def mock_find_spec():
    """模拟 importlib.util.find_spec"""
    with patch('byksdk._internal.find_spec') as mock:
        yield mock