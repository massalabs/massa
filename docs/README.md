## Setup

python -m venv venv
venv/bin/python -m pip install -r requirements.txt

## Build doc locally

[Edit Makefile] change the following line from:

* SPHINXBUILD   ?= sphinx-build

TO

* SPHINXBUILD   ?= venv/bin/sphinx-build

then run:

```commandline
make html
```

