#!/usr/bin/env python3
from setuptools import setup

try:
    from setuptools_rust import Binding, RustExtension
except ImportError:
    import subprocess

    errno = subprocess.call([
        '/usr/bin/env', 'python3', '-m', 'pip', 'install', 'setuptools-rust'
    ])
    if errno:
        print('Please install setuptools-rust package')
        raise SystemExit(errno)
    else:
        from setuptools_rust import Binding, RustExtension

setup(
    name='taxonomy',
    version='0.1.0',
    author='One Codex',
    rust_extensions=[
        RustExtension(
            'taxonomy',
            'Cargo.toml',
            features=['python'],
            debug=False,
            binding=Binding.PyO3
        ),
    ],
    packages=[],
    setup_requires=['setuptools-rust>=0.10.1', 'wheel'],
    install_requires=[],
    # rust extensions are not zip safe, just like C-extensions.
    zip_safe=False
)
