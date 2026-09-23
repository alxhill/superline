Wait+Screen /\x{F105}/
Screenshot first.png
Type "echo hello"
Enter
Wait+Screen /\nhello\n(?s:.*)\x{F105}/
Screenshot second.png
