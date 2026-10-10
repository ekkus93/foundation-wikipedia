#!/usr/bin/env python3
"""Stage one verified Wikimedia current-content XML export as NDJSON."""
import argparse
from stage_public_current_content import stage_verified_pages


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source")
    parser.add_argument("sha256")
    parser.add_argument("target")
    args = parser.parse_args()
    print(stage_verified_pages(args.source, args.sha256, args.target))


if __name__ == "__main__":
    main()
