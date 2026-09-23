Wait+Screen /\x{F105}/
Screenshot prompt.png
Type "echo hi"
Enter
Wait+Screen /\nhi\n(?s:.*)\x{F105}/
Screenshot after.png
