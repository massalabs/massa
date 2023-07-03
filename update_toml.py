# TODO    Update dev-dependencies as well

import os
import copy
import toml
import json
from glob import glob

import re

inline_pattern = re.compile(r"\[dependencies\.(.*)\]\nworkspace\s=\strue")

def get_dependencies(data):
    new_deps = {}
    for dep, depd in data.items():
        add_flag = False
        if dep not in all_deps:
            all_deps[dep] = depd
        else:
            if not check_compatible(all_deps[dep], depd):
                # print(f"Skipping {dep} (not compatible)")
                add_flag = True
                # new_deps[dep] = depd
                # continue

        new_depd = { "workspace": True, "flag": add_flag, "old": depd}
        print(type(new_depd))
        if isinstance(depd, dict):
            if "features" in depd:
                new_depd["features"] = depd["features"]
            if "optional" in depd:
                new_depd["optional"] = depd["optional"]
        print(dep, depd, new_depd)
        new_deps[dep] = new_depd
    return new_deps

def inline_dict_fmt(d):
    tail = ""
    if "flag" in d:
        if d["flag"]:
            tail += "   # BOM UPGRADE     Revert to {} if problem".format(
                json.dumps(d["old"])
            )
        del d["flag"]
        del d["old"]
        
    if not isinstance(d, dict):
        return json.dumps(d)
    
    inldict = json.dumps(d).replace(": ", " = ")
    res = ""
    is_val = False
    print(inldict)
    for x in inldict.split():
        if len(x) == 1:
            if x == "=":
                is_val = True
            elif x == ",":
                is_val = False
            res += x
        else:
            if not is_val:
                res += x.replace('"', "")
            else:
                res += x
        res += " "
    print(res)
    assert is_val
    return res[:-1] + tail

def check_compatible(workspace, dep):
    if not isinstance(dep, dict):
        if not isinstance(workspace, dict):
            return dep == workspace
        else:
            if "version" in workspace:
                return dep == workspace["version"]
            else:
                return False
    else:
        if not isinstance(workspace, dict):
            return False
        
        if "version" in dep:
            return ("version" in workspace) and (dep["version"] == workspace["version"])

        if ("git" in dep) and ("git" in workspace):
            if dep["git"] != workspace["git"]:
                return False

            if ("rev" in dep) and ("rev" in workspace):
                return dep["rev"] == workspace["rev"]
            if ("branch" in dep) and ("branch" in workspace):
                return dep["branch"] == workspace["branch"]

            return False
        if "path" in dep:
            if "path" in workspace:
                return dep["path"] == workspace["path"]
            else:
                return True
        if "workspace" in dep:
            return dep["workspace"]

        print("Got unexpected case")
        print("workspace", workspace)
        print("dep", dep)
        breakpoint()

def update_cargo_toml(all_deps, fpath):
    print(f"Treat {fpath}")
    with open(fpath, "r") as f:
        data = toml.load(f)

    tail = ""
    if "dependencies" in data:
        new_deps = get_dependencies(data["dependencies"])
        del data["dependencies"]
        tail += "\n[dependencies]\n"
        for (dep, depd) in new_deps.items():
            print(depd)
            depd_fmt = inline_dict_fmt(depd)
            tail += f"{dep} = {depd_fmt}\n"

    if "dev-dependencies" in data:
        new_dev_deps = get_dependencies(data["dev-dependencies"])
        del data["dev-dependencies"]

        tail += "\n[dev-dependencies]\n"
        for (dep, depd) in new_dev_deps.items():
            depd_fmt = inline_dict_fmt(depd)
            tail += f"{dep} = {depd_fmt}\n"

    res = toml.dumps(data, encoder=toml.TomlPreserveCommentEncoder())
    res += tail
    res = res.replace(",]", "]").replace("[ ", "[")
    with open(fpath, "w") as f:
        f.write(res)

all_deps = {}
for path in glob("./*", recursive = False):
    if os.path.isdir(path) and os.path.isfile(os.path.join(path, "Cargo.toml")):
        update_cargo_toml(all_deps, os.path.join(path, "Cargo.toml"))

deps = list(all_deps.keys())
deps = sorted(deps)
with open("./Cargo.toml", "a") as f:
    f.write("[workspace.dependencies]\n")
    for dep in deps:
        depd = all_deps[dep]
        if "optional" in depd:
            del depd["optional"]
        if "features" in depd:
            del depd["features"]
        depd_fmt = inline_dict_fmt(depd).replace("../", "./")
        f.write(f"{dep} = {depd_fmt}\n")