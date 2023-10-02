# import tomllib
import os
import re
import shutil
import subprocess
import tomllib
from pathlib import Path
import argparse

def coverage_for_pkg(package: str, capture_output: bool = False):

    package_ = re.sub("-", "_", package)

    base_output = Path("coverage") / package_
    if base_output.exists():
        print("Removing existing folder:", base_output)
        shutil.rmtree(base_output)

    print("Creating folder:", base_output)
    base_output.mkdir(parents=True)

    enable_feature_testing: bool = False
    with open(f"{package}/Cargo.toml", "rb") as f:
        data = tomllib.load(f)
        enable_feature_testing = "features" in data and "testing" in data["features"]

    base_cmd = [
        "cargo",
        "llvm-cov",
        "--html",
        f"--output-dir {base_output}",
        "test",
        # "--open",
        f"--package {package_}",
    ]

    if enable_feature_testing:
        base_cmd.append("--features testing")

    ignore_flag = ["--ignore-filename-regex"]
    ignore_flag_arg_ = []
    for i, folder in enumerate(os.listdir("./")):
        # print("folder", folder)
        if os.path.isdir(folder) and re.search("^(massa-)", folder) and folder != package:
            ignore_flag_arg_.append(f"({folder})")

    # print("ignore_flag_arg_", ignore_flag_arg_)
    ignore_flag_arg_.append("(test_exports)")
    ignore_flag_arg_.append("(test_helpers)")

    final_cmd = base_cmd[:]
    final_cmd.extend(ignore_flag)
    ignore_flag_arg = "|".join(ignore_flag_arg_)
    # print("ignore_flag_arg", ignore_flag_arg)
    final_cmd.append(f'"{ignore_flag_arg}"')
    print("final_cmd:", " ".join(final_cmd))

    if capture_output:
        p = subprocess.run(" ".join(final_cmd), shell=True, check=True, capture_output=True)
        if p.returncode != 0:
            print(f"Error:\nstdout:\n{p.stdout}\nstderr:\n{p.stderr}")
    else:
        subprocess.run(" ".join(final_cmd), shell=True, check=True)
    print("Report:", base_output / Path(package_) / "html" / "index.html")


if __name__ == "__main__":

    parser = argparse.ArgumentParser(
        prog='llvm-cov-pkg',
        description='Generate coverage using llvm-cov with correct filters',
    )
    parser.add_argument("-p", "--package", help="Generate coverage for only one workspace member")
    parser.add_argument("-a", "--all", action="store_true", help="Generate coverage for all workspace members")
    parser.add_argument("-c", "--clean", action="store_true", help="Clean coverage data")
    args = parser.parse_args()

    if args.all:
        with open("Cargo.toml", "rb") as f:
            data = tomllib.load(f)
            for pkg in data["workspace"]["members"]:
                # print("p", pkg, type(pkg))
                if pkg not in ["massa-client", "massa-node", "massa-xtask"]:
                    coverage_for_pkg(pkg, capture_output=True)
                else:
                    print(f"Skipping package: {pkg}")
    elif args.package:
        coverage_for_pkg(args.package)
    elif args.clean:
        # find . -name "*.profraw" -delete
        subprocess.run("cargo llvm-cov clean --workspace", shell=True, check=True)
    else:
        print("Need --package or --all as argument, ex: --package massa-async-pool")
