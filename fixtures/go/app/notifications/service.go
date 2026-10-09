package notifications

// Outbound notices.

type NotificationService struct{}

func (n *NotificationService) Send(userID string, message string) string {
	return message
}
