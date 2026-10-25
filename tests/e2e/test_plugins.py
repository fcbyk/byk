"""E2E 插件生命周期测试。

byk.json 插件系统（install_plugin）已废弃——远程分发改为拉取 *.byk.json、
本地安装改为 byk add <file>。原 python/python-m 插件测试（依赖 command/downloads
等 byk.json 字段）随入口废弃而删除，待插件迁移到 *.byk.json 后以等价形式
（$cmd + $pip + $assets）重新覆盖。
"""
