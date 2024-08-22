// src/secureApp.ts

function initializeApiClient() {
  const baseUrl = process.env.QUOTES_API_BASE_URL!
  const token = process.env.QUOTES_API_TOKEN!

  console.log(`API Client initialized with base URL: ${baseUrl}, API Version: ${apiVersion}`)

  return {
    fetchResource: async (endpoint: string) => {
      const response = await fetch(`${baseUrl}${apiVersion}/${endpoint}`, {
        headers: {
          Authorization: `Bearer ${token}`,
        },
      })
      return response.json()
    },
  }
}

async function fetchData() {
  const apiKey = process.env.WEATHER_API_KEY!
  const clientId = process.env.WEATHER_API_CLIENT_ID!

  console.log(`Fetching data with API key ${apiKey} and client ID ${clientId}`)
  const response = await fetch('https://api.weather.com/resource', {
    headers: {
      Authorization: `Bearer ${apiKey}`,
      'Client-ID': clientId,
    },
  })
  const data = await response.json()
  return data
}

async function main() {
  const apiClient = initializeApiClient()

  const dataFromClient = await apiClient.fetchResource('data')
  console.log(`Fetched data from API client: ${JSON.stringify(dataFromClient)}`)

  const dataFromFunction = await fetchData()
  console.log(`Fetched data from fetchData function: ${JSON.stringify(dataFromFunction)}`)
}

main()
