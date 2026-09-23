# Uses the shared tests/terminal/config.json.
Wait+Screen /\x{F105}/
Screenshot clean.png
Type "sl-test exit 7"
Enter
Wait+Screen /\x{E0B0}7\x{E0B0}/
Screenshot failure.png
