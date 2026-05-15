package oris

import (
	"fmt"

	"github.com/Colin4k1024/Oris/sdks/go/experience"
	"github.com/Colin4k1024/Oris/sdks/go/hub"
	"github.com/Colin4k1024/Oris/sdks/go/store"
	orissync "github.com/Colin4k1024/Oris/sdks/go/sync"
)

type HubSync struct {
	BaseURL string
	APIKey  string
	Seed    [32]byte
	NodeID  string
}

type ExperienceSync struct {
	BaseURL  string
	APIKey   string
	Seed     [32]byte
	SenderID string
}

type Config struct {
	StorePath  string
	Hub        *HubSync
	Experience *ExperienceSync
}

type OrisClient struct {
	Store *store.LocalStore
	Sync  *orissync.SyncManager
}

func NewClient(cfg Config) (*OrisClient, error) {
	storePath := cfg.StorePath
	if storePath == "" {
		storePath = "oris_genes.db"
	}

	ls, err := store.Open(storePath)
	if err != nil {
		return nil, fmt.Errorf("open local store: %w", err)
	}

	syncCfg := orissync.Config{Store: ls}

	if cfg.Experience != nil {
		syncCfg.Experience = experience.NewClient(experience.Config{
			BaseURL:  cfg.Experience.BaseURL,
			APIKey:   cfg.Experience.APIKey,
			Seed:     cfg.Experience.Seed,
			SenderID: cfg.Experience.SenderID,
		})
	}

	if cfg.Hub != nil {
		syncCfg.Hub = hub.NewClient(hub.Config{
			BaseURL: cfg.Hub.BaseURL,
			APIKey:  cfg.Hub.APIKey,
			Seed:    cfg.Hub.Seed,
			NodeID:  cfg.Hub.NodeID,
		})
	}

	return &OrisClient{
		Store: ls,
		Sync:  orissync.New(syncCfg),
	}, nil
}

func (c *OrisClient) Close() error {
	return c.Store.Close()
}
