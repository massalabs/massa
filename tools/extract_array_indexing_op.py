import sys
import re

def index_to_coordinates(s, index):
    """Returns (line_number, col) of `index` in `s`."""
    if not len(s):
        return 1, 1
    sp = s[:index+1].splitlines(keepends=True)
    return len(sp), len(sp[-1])

to_exclude = [
    "[test]", # unit test
    "[serial]", # unit test
    "[inline]", # optim :-D
    "[must_use]",
    "[dependencies]", # toml found in rust code ...
    "[dev-dependencies]",
    "[non_exhaustive]", # Non non_exhaustive enum definition
    "[from]", # thiserror syntax
    "[u8]", # u8 array type
    "[..]", # index all
    "#[serde_as]" # Serde
]

if __name__ == "__main__":

    filepath = sys.argv[1]
    print(f"Opening file: {filepath}...")
    match_count = 0
    with open(filepath) as fp:
        file_content = fp.read()
        for m in re.finditer(r"\[([\w:\+\.-]+)\]", file_content, flags = re.IGNORECASE | re.MULTILINE):

            if m.group(0) in to_exclude:
                continue

            line_number, col = index_to_coordinates(file_content, m.span()[0])
            print(f"Found match {m} at line {line_number}")


            match_count += 1

    if match_count == 0:
        print("Nothing found")