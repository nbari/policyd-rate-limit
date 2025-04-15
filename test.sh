cat <<EOF | socat - UNIX-CONNECT:/tmp/policyd.sock
request=smtpd_access_policy
protocol_state=RCPT
protocol_name=SMTP
helo_name=mail.example.com
queue_id=ABC123DEF456
sender=user@example.com
recipient=to@example.net
client_address=1.2.3.4
client_name=mail.example.com
instance=xyz789
sasl_method=PLAIN
sasl_username=authenticated-user@example.com
sasl_sender=user@example.com

EOF
