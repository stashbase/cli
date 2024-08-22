function initializeApiClient() {
  const baseUrl = 'https://api.quotes.com/v1'
  const token = '9b8b7823b48b0e92390'

  console.log(`API Client initialized with base URL: ${baseUrl}`)

  return {
    fetchResource: async (endpoint: string) => {
      const response = await fetch(`${baseUrl}/${endpoint}`, {
        headers: {
          Authorization: `Bearer ${token}`,
        },
      })
      return response.json()
    },
  }
}

async function fetchData() {
  const apiKey = 'wh_xeC39HqLyjWDarjtT1zdp7dc'
  const clientId = 'whether-api-client-845u345690'

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
