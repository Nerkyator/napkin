#!/usr/bin/env python3
"""Build crates/engine/data/cities.tsv from GeoNames cities15000.

Source: https://download.geonames.org/export/dump/cities15000.zip
License: GeoNames data is CC BY 4.0 (https://www.geonames.org/).

Output format:
  line 1: every IANA zone used, tab separated
  then:   <lowercase city name>\t<index of its zone in line 1>

Each city contributes its name and ASCII name; cities with at least
ALIAS_MIN_POPULATION inhabitants also contribute their ASCII alternate names
(so "New York" finds New York City). When two cities share a name, the more
populous one wins: "paris" is Paris, France.
"""
import re
import sys
import zipfile

ALIAS_MIN_POPULATION = 300_000
ALIAS_OK = re.compile(r"^[A-Za-z][A-Za-z .'\-]*[A-Za-z.]$")


def main(zip_path: str, out_path: str) -> None:
    text = zipfile.ZipFile(zip_path).read("cities15000.txt").decode("utf-8")
    best: dict[str, tuple[int, str]] = {}
    for line in text.splitlines():
        f = line.split("\t")
        population = int(f[14] or 0)
        zone = f[17]
        if not zone:
            continue
        names = {f[1], f[2]}
        if population >= ALIAS_MIN_POPULATION:
            for alias in f[3].split(","):
                if ALIAS_OK.match(alias) and not alias.isupper() and len(alias) >= 3:
                    names.add(alias)
        for name in names:
            key = name.strip().lower()
            if key and (key not in best or best[key][0] < population):
                best[key] = (population, zone)
    zones = sorted({zone for _, zone in best.values()})
    index = {zone: i for i, zone in enumerate(zones)}
    with open(out_path, "w", encoding="utf-8") as out:
        out.write("\t".join(zones) + "\n")
        for key in sorted(best):
            out.write(f"{key}\t{index[best[key][1]]}\n")
    print(f"{len(best)} names, {len(zones)} zones -> {out_path}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
