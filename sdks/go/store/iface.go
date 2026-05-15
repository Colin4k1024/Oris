package store

import (
	"context"
	"time"
)

type Store interface {
	Close() error
	Save(ctx context.Context, gene Gene) error
	Get(ctx context.Context, geneID string) (*Gene, error)
	Delete(ctx context.Context, geneID string) error
	Query(ctx context.Context, q StoreQuery) ([]Gene, error)
	UpdateStats(ctx context.Context, geneID string, used bool, success bool) error
	List(ctx context.Context, opts ListOpts) ([]Gene, error)
	GetUnsynced(ctx context.Context) ([]Gene, error)
	MarkSynced(ctx context.Context, geneID string, syncedAt time.Time) error
	LogSync(ctx context.Context, entry SyncLogEntry) error
	GetSyncLog(ctx context.Context, limit int) ([]SyncLogEntry, error)
}
